<script lang="ts">
  import { get, message, send, type Category, type Chapter, type Namesake, type PageMark, type Title } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { canKeep, forget as forgetHere, keep as keepHere, keptChapters } from '../lib/offline';
  import { pref, setPref } from '../lib/prefs.svelte';
  import { back, go } from '../lib/router.svelte';
  import Tracking from '../lib/Tracking.svelte';
  import { ago, chapterShort, langName, STATUS } from '../lib/util';

  const WEEK = 7 * 86400;

  type Sort = 'source' | 'number' | 'date';
  /** `volumes` puts a heading over each volume's chapters. */
  type View = { sort: Sort; down: boolean; volumes: boolean };

  const SORTS: [Sort, string][] = [
    ['source', 'Source order'],
    ['number', 'Number'],
    ['date', 'Date'],
  ];
  const DEFAULT: View = { sort: 'source', down: true, volumes: false };

  type Show = 'all' | 'unread' | 'downloaded' | 'bookmarked';

  const SHOWS: [Show, string][] = [
    ['all', 'All chapters'],
    ['unread', 'Unread'],
    ['downloaded', 'Downloaded'],
    ['bookmarked', 'Bookmarked'],
  ];

  let { id }: { id: number } = $props();

  /** The title last open, if it is this one: come back to from the reader, it shows at once. */
  function before(): Title | null {
    const was = recall<Title>('title');
    return was?.id === id ? was : null;
  }

  let title = $state<Title | null>(before());
  let loading = $state(true);
  let error = $state<string | null>(null);
  // A failed refresh keeps what is on screen and says so, instead of replacing it.
  let refreshing = $state(false);
  let refreshError = $state<string | null>(null);
  let menu = $state(false);
  let wholeText = $state(false);
  let filters = $state(false);
  // The chapter whose own menu is open.
  let opened = $state<number | null>(null);
  // Picking chapters to act on several at once.
  let picking = $state(false);
  let picked = $state<Record<number, boolean>>({});
  // The chapter picked last: a long press or a shift-click on another takes in every chapter between.
  let anchor = $state<number | null>(null);
  let sure = $state(false);
  // What the list is narrowed to lasts as long as the page: the next title opened shows whole.
  let show = $state<Show>('all');
  let group = $state('');
  // The uploads a title that shows one per chapter does not prefer, shown anyway.
  let others = $state(false);
  // Library titles of the same name, found when this one was about to be added.
  let namesakes = $state<Namesake[] | null>(null);
  // The panels that open over the about column.
  let panel = $state<'edit' | 'groups' | null>(null);
  let marks = $state<PageMark[]>([]);
  // Chapters kept on this device, and how keeping more is going.
  let here = $state<Set<number>>(new Set());
  let keeping = $state('');

  // A title that shows one upload per chapter counts chapters, not uploads.
  const chapters = $derived((title?.chapters ?? []).filter((c) => others || !c.duplicate));
  const unread = $derived(chapters.filter((c) => !c.read).length);
  const saved = $derived(chapters.filter((c) => c.downloaded).length);
  const duplicates = $derived((title?.chapters ?? []).filter((c) => c.duplicate).length);
  const merged = $derived((title?.members.length ?? 0) > 1);
  // The order is kept for every title, on the server like the library's.
  const view = $derived<View>({ ...DEFAULT, ...pref<Partial<View>>('title.view', {}) });
  const groups = $derived([...new Set(chapters.map((c) => c.scanlator ?? '').filter(Boolean))].sort((a, b) => a.localeCompare(b)));
  // A group a refresh took away narrows nothing.
  const from = $derived(groups.includes(group) ? group : '');
  const filtering = $derived(show !== 'all' || from !== '');
  const listed = $derived.by(() => {
    const shown: Record<Show, (c: Chapter) => boolean> = { all: () => true, unread: (c) => !c.read, downloaded: (c) => c.downloaded, bookmarked: (c) => c.bookmark };
    const list = chapters.filter((c) => shown[show](c) && (!from || c.scanlator === from));
    // Sources list the newest first, and so does every order here until it is turned round.
    if (view.sort !== 'source') {
      const key = view.sort === 'number' ? (c: Chapter) => c.number : (c: Chapter) => c.dateUpload || c.fetchedAt * 1000;
      list.sort((a, b) => key(b) - key(a));
    }
    return view.down ? list : list.reverse();
  });
  // Only what is shown is acted on: a chapter the filter hides drops out.
  const chosen = $derived(listed.filter((c) => picked[c.id]));
  const allKept = $derived(chosen.length > 0 && chosen.every((c) => c.bookmark));
  const nothing = $derived(
    !chapters.length ? 'No chapters yet.' : show === 'unread' && !from ? 'Every chapter is read.' : 'No chapter fits the filter.',
  );
  // Reading goes from the bottom of the list up: the next one to read is the last unread.
  const resume = $derived(chapters.findLast((c) => !c.read) ?? null);
  const started = $derived(chapters.some((c) => c.read || c.lastPageRead > 0));

  async function load() {
    loading = !title;
    error = null;
    try {
      const fresh = await get<Title>(`/manga/${id}`);
      // A title merged into another is read there.
      if (fresh.mergedInto) return go(`/title/${fresh.mergedInto}`, true);
      keep('title', fresh);
      title = fresh;
      // A title only seen in a list has no details or chapters yet.
      if (!title.initialized || title.chapters.length === 0) refresh();
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  // Wanted, not needed: kept pages and what is on this device show once they are known.
  function extras() {
    get<PageMark[]>(`/manga/${id}/marks`)
      .then((found) => (marks = found))
      .catch(() => {});
    keptChapters()
      .then((found) => (here = found))
      .catch(() => {});
  }
  extras();

  async function refresh() {
    menu = false;
    refreshing = true;
    refreshError = null;
    try {
      title = await send<Title>('POST', `/manga/${id}/refresh`);
    } catch (e) {
      refreshError = message(e);
    }
    refreshing = false;
  }

  let allCategories = $state<Category[]>([]);
  get<{ categories: Category[] }>('/categories')
    .then((answer) => (allCategories = answer.categories))
    .catch(() => {});

  function toggleCategory(category: number) {
    if (!title) return;
    const t = title;
    t.categories = t.categories.includes(category) ? t.categories.filter((c) => c !== category) : [...t.categories, category];
    change(() => send('PUT', `/manga/${id}/categories`, { ids: t.categories }), true);
  }

  let note = $state('');

  function download(list: Chapter[]) {
    change(async () => {
      const answer = await send<{ queued: number }>('POST', '/downloads', { chapters: list.map((c) => c.id) });
      note = answer.queued ? `${answer.queued} ${answer.queued === 1 ? 'chapter' : 'chapters'} queued for download.` : 'Already saved or queued.';
    });
  }

  function deleteFiles(list: Chapter[]) {
    for (const chapter of list) chapter.downloaded = false;
    change(() => Promise.all(list.map((c) => send('DELETE', `/chapters/${c.id}/download`))));
  }

  function toggleAuto() {
    if (!title) return;
    const t = title;
    t.autoDownload = !t.autoDownload;
    change(() => send('PUT', `/manga/${id}/auto-download`, { on: t.autoDownload }), true);
  }

  async function change(work: () => Promise<unknown>, keepMenu = false) {
    menu = keepMenu && menu;
    opened = null;
    refreshError = null;
    try {
      await work();
    } catch (e) {
      refreshError = message(e);
      title = await get<Title>(`/manga/${id}`).catch(() => title);
    }
  }

  /** Adds the title to the library or takes it out. Before adding, a title of the same name already there is pointed out. */
  async function toggleLibrary(anyway = false) {
    if (!title) return;
    const t = title;
    if (!t.inLibrary && !anyway) {
      const found = await get<Namesake[]>(`/manga/${id}/namesakes`).catch(() => []);
      if (found.length) {
        namesakes = found;
        return;
      }
    }
    namesakes = null;
    t.inLibrary = !t.inLibrary;
    change(() => send('PUT', `/manga/${id}/library`, { inLibrary: t.inLibrary }));
  }

  /** Reads this title from under another, as one of its sources. */
  async function mergeInto(head: number) {
    try {
      await send('POST', `/manga/${head}/merge/${id}`);
      go(`/title/${head}`, true);
    } catch (e) {
      refreshError = message(e);
    }
  }

  function unmerge(member: number) {
    change(async () => (title = await send<Title>('DELETE', `/manga/${id}/merge/${member}`)));
  }

  // ---------- details edited by hand ----------

  let form = $state({ title: '', author: '', artist: '', description: '', genres: '', status: 0 });

  function startEdit() {
    if (!title) return;
    menu = false;
    form = {
      title: title.title,
      author: title.author ?? '',
      artist: title.artist ?? '',
      description: title.description ?? '',
      genres: title.genres.join(', '),
      status: title.status,
    };
    panel = 'edit';
  }

  /** Sends only what was changed: each field sent is locked against the source. */
  function saveEdit(event: SubmitEvent) {
    event.preventDefault();
    if (!title) return;
    const t = title;
    const genres = form.genres.split(',').map((g) => g.trim()).filter(Boolean);
    const body: Record<string, unknown> = {};
    if (form.title.trim() !== t.title) body.title = form.title;
    if (form.author.trim() !== (t.author ?? '')) body.author = form.author;
    if (form.artist.trim() !== (t.artist ?? '')) body.artist = form.artist;
    if (form.description.trim() !== (t.description ?? '').trim()) body.description = form.description;
    if (genres.join(',') !== t.genres.join(',')) body.genres = genres;
    if (form.status !== t.status) body.status = form.status;
    panel = null;
    if (Object.keys(body).length) change(async () => (title = await send<Title>('PUT', `/manga/${id}/details`, body)));
  }

  function unlock() {
    panel = null;
    refreshing = true;
    change(async () => (title = await send<Title>('DELETE', `/manga/${id}/details`))).finally(() => (refreshing = false));
  }

  async function setCover(file: File | undefined) {
    if (!file) return;
    refreshError = null;
    try {
      const answer = await fetch(`api/manga/${id}/cover`, { method: 'PUT', headers: { 'Content-Type': file.type || 'application/octet-stream' }, body: file });
      const data = await answer.json().catch(() => null);
      if (!answer.ok) throw new Error(data?.error?.message ?? `Mango answered ${answer.status}`);
      title = data as Title;
    } catch (e) {
      refreshError = message(e);
    }
  }

  // ---------- scanlation groups ----------

  let order = $state<string[]>([]);
  let dedupe = $state(false);

  function startGroups() {
    if (!title) return;
    menu = false;
    const named = title.scanlators.filter((g) => groups.includes(g));
    order = [...named, ...groups.filter((g) => !named.includes(g))];
    dedupe = title.dedupe;
    panel = 'groups';
  }

  function moveGroup(at: number, by: number) {
    const next = [...order];
    const [group] = next.splice(at, 1);
    next.splice(at + by, 0, group);
    order = next;
  }

  function saveGroups() {
    panel = null;
    change(async () => (title = await send<Title>('PUT', `/manga/${id}/scanlators`, { order, dedupe })));
  }

  // ---------- kept pages and this device ----------

  async function unmarkPage(mark: PageMark) {
    marks = marks.filter((m) => m !== mark);
    await send('DELETE', `/chapters/${mark.chapterId}/marks/${mark.page}`).catch((e) => (refreshError = message(e)));
  }

  async function keepOnDevice(list: Chapter[]) {
    keeping = 'Keeping…';
    try {
      const done = await keepHere(id, list.map((c) => c.id), (pages) => (keeping = `Keeping… ${pages} pages so far`));
      keeping = `${done} ${done === 1 ? 'chapter is' : 'chapters are'} on this device, to read with no connection.`;
      here = await keptChapters();
    } catch (e) {
      keeping = `Not all of it was kept: ${message(e)}`;
    }
  }

  async function forgetOnDevice(list: Chapter[]) {
    await forgetHere(list.map((c) => c.id)).catch(() => {});
    here = await keptChapters().catch(() => here);
  }

  function mark(list: Chapter[], read: boolean) {
    for (const chapter of list) {
      chapter.read = read;
      chapter.lastPageRead = 0;
    }
    change(() => send('PUT', `/manga/${id}/chapters`, { ids: list.map((c) => c.id), read }));
  }

  /** The chapters older than this one, in the source's order, that are still unread. */
  function unreadBefore(chapter: Chapter): Chapter[] {
    return chapters.slice(chapters.indexOf(chapter) + 1).filter((c) => !c.read);
  }

  function markUpTo(chapter: Chapter) {
    mark([...(chapter.read ? [] : [chapter]), ...unreadBefore(chapter)], true);
  }

  function setBookmark(list: Chapter[], on: boolean) {
    for (const chapter of list) chapter.bookmark = on;
    change(() => send('PUT', `/manga/${id}/chapters`, { ids: list.map((c) => c.id), bookmark: on }));
  }

  function startPicking(chapter?: Chapter) {
    opened = null;
    picking = true;
    picked = chapter ? { [chapter.id]: true } : {};
    anchor = chapter?.id ?? null;
    sure = false;
  }

  function stopPicking() {
    picking = false;
    picked = {};
    anchor = null;
    sure = false;
  }

  /** Picks every chapter shown from the one picked last to this one. */
  function pickTo(chapter: Chapter) {
    const ids = listed.map((c) => c.id);
    const end = ids.indexOf(chapter.id);
    const start = anchor === null || !ids.includes(anchor) ? end : ids.indexOf(anchor);
    for (const id of ids.slice(Math.min(start, end), Math.max(start, end) + 1)) picked[id] = true;
    anchor = chapter.id;
    sure = false;
  }

  /** While picking, a click on a row picks it instead of opening it. A shift-click on a chapter picks a run, and starts picking. */
  function pick(event: MouseEvent, chapter: Chapter) {
    if (held) {
      held = false;
      event.preventDefault();
      event.stopPropagation();
      return;
    }
    if (!picking && !(event.shiftKey && (event.target as Element).closest?.('.main'))) return;
    event.preventDefault();
    event.stopPropagation();
    if (!picking) startPicking(chapter);
    else if (event.shiftKey) pickTo(chapter);
    else {
      picked[chapter.id] = !picked[chapter.id];
      anchor = chapter.id;
      sure = false;
    }
  }

  // A long press picks, as in Mihon: the first starts picking, the next takes in the run up to it.
  let press: ReturnType<typeof setTimeout> | undefined;
  let pressAt = { x: 0, y: 0 };
  // A long press just ended: the click that follows it is not a pick.
  let held = false;

  function pressStart(event: PointerEvent, chapter: Chapter) {
    held = false;
    if (event.button !== 0) return;
    pressAt = { x: event.clientX, y: event.clientY };
    clearTimeout(press);
    press = setTimeout(() => {
      press = undefined;
      held = true;
      navigator.vibrate?.(15);
      if (picking) pickTo(chapter);
      else startPicking(chapter);
    }, 450);
  }

  function pressMove(event: PointerEvent) {
    if (press && Math.hypot(event.clientX - pressAt.x, event.clientY - pressAt.y) > 10) pressEnd();
  }

  function pressEnd() {
    clearTimeout(press);
    press = undefined;
  }

  /** Runs an action on the picked chapters, then stops picking. */
  function onPicked(action: (list: Chapter[]) => void) {
    action(chosen);
    stopPicking();
  }

  function deletePicked() {
    if (sure) onPicked((list) => deleteFiles(list.filter((c) => c.downloaded)));
    else sure = true;
  }

  function setView(patch: Partial<View>) {
    setPref('title.view', { ...view, ...patch }).catch((e) => (refreshError = message(e)));
  }

  /** "Weekly", from the days a title usually leaves between two chapters. */
  function pace(days: number): string {
    return days === 1 ? 'Daily' : days === 7 ? 'Weekly' : days >= 28 ? 'Monthly or slower' : `Every ${days} days`;
  }

  function about(chapter: Chapter): string {
    const when = chapter.dateUpload > 0 ? ago(chapter.dateUpload / 1000) : chapter.fetchedAt > 0 ? `found ${ago(chapter.fetchedAt)}` : '';
    const where = chapter.read
      ? 'read'
      : chapter.lastPageRead > 0
        ? `page ${chapter.lastPageRead + 1}${chapter.pageCount ? ` of ${chapter.pageCount}` : ''}`
        : chapter.pageCount
          ? `${chapter.pageCount} pages`
          : '';
    const from = merged ? title?.members.find((m) => m.id === chapter.mangaId)?.sourceName : null;
    return [when, from, chapter.scanlator, where, chapter.downloaded ? 'saved' : '', here.has(chapter.id) ? 'on this device' : '', chapter.duplicate ? 'other upload' : '']
      .filter(Boolean)
      .join(' · ');
  }

  const volumeName = (volume: number | null) => (volume === null ? 'No volume' : `Volume ${+volume.toFixed(2)}`);

  function isNew(chapter: Chapter): boolean {
    return !chapter.read && chapter.lastPageRead === 0 && !!title?.addedAt && chapter.fetchedAt > title.addedAt && Date.now() / 1000 - chapter.fetchedAt < WEEK;
  }
</script>

<!-- A chapter's menu closes when anything else is pressed. -->
<svelte:window
  onclick={(e) => opened !== null && !(e.target as Element).closest?.('.own') && (opened = null)}
  onkeydown={(e) => e.key === 'Escape' && picking && stopPicking()}
/>

<div class="page" class:picking>
  <Notice {loading} {error} retry={load} />

  {#if title}
    <div class="bar">
      <a class="btn icon lift" href="#/" aria-label="Back" title="Back" onclick={(e) => (e.preventDefault(), back())}>
        <Icon name="back" stroke={3} />
      </a>
      <div class="tools">
        <button
          class="btn lift spot-on"
          aria-pressed={title.inLibrary}
          onclick={() => toggleLibrary()}
        >
          <Icon name="bookmark" fill={title.inLibrary} />{title.inLibrary ? 'In library' : 'Add to library'}
        </button>
        <div class="menu">
          <button class="btn icon lift" aria-label="More actions" title="More actions" aria-expanded={menu} onclick={() => (menu = !menu)}><Icon name="more" fill /></button>
          {#if menu}
            <div class="panel sheet">
              <button class="btn" onclick={refresh}><Icon name="refresh" size={18} />Refresh from the source</button>
              <button class="btn" onclick={() => mark(title?.chapters ?? [], true)}><Icon name="check" size={18} />Mark all read</button>
              <button class="btn" onclick={() => mark(title?.chapters ?? [], false)}>Mark all unread</button>
              <button class="btn" onclick={startEdit}><Icon name="sliders" size={18} />Edit details</button>
              {#if groups.length > 1 || merged}
                <button class="btn" onclick={startGroups}>Scanlation groups</button>
              {/if}
              <a class="btn" href="#/search?q={encodeURIComponent(title.title)}&for={id}"><Icon name="arrow" size={18} />Move to another source</a>
              <a class="btn" href="#/search?q={encodeURIComponent(title.title)}&merge={id}"><Icon name="plus" size={18} />Add another source</a>
              {#if title.inLibrary}
                <button class="btn" aria-pressed={title.autoDownload} onclick={toggleAuto}>
                  {#if title.autoDownload}<Icon name="check" size={18} stroke={3} />{/if}Download new chapters
                </button>
              {/if}
              {#if title.inLibrary && allCategories.length}
                <div class="cats" role="group" aria-label="Categories">
                  <span>Categories</span>
                  {#each allCategories as category (category.id)}
                    {@const inIt = title.categories.includes(category.id)}
                    <button class="btn" aria-pressed={inIt} onclick={() => toggleCategory(category.id)}>
                      {#if inIt}<Icon name="check" size={18} stroke={3} />{/if}{category.name}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>

    {#if namesakes?.length}
      <div class="panel warn" role="alert">
        <strong>Already in your library, from another source:</strong>
        <ul class="rows">
          {#each namesakes as twin (twin.id)}
            <li>
              <a class="main" href="#/title/{twin.id}"><strong>{twin.title}</strong><span>{twin.sourceName ?? 'Its extension is not installed'}</span></a>
              <button class="btn" onclick={() => mergeInto(twin.id)}>Read this as one of its sources</button>
            </li>
          {/each}
        </ul>
        <div class="tools">
          <button class="btn ink" onclick={() => toggleLibrary(true)}>Add anyway</button>
          <button class="btn" onclick={() => (namesakes = null)}>Cancel</button>
        </div>
      </div>
    {/if}

    <div class="columns">
      <div class="about">
        <div class="hero">
          <div class="art"><Cover src={title.cover} title={title.title} tilt /></div>
          <div class="facts">
            <div class="tools">
              <span class="tag ink">{STATUS[title.status] ?? 'Unknown'}</span>
              {#if title.sourceLang}<span class="tag">{langName(title.sourceLang)}</span>{/if}
              {#if title.releaseDays && title.status !== 2 && title.status !== 4}
                <span class="tag" title="How often it gets a chapter, going by the source's dates">{pace(title.releaseDays)}</span>
              {/if}
            </div>
            <h1 class="display">{title.title}</h1>
            <strong>{title.sourceName ?? 'Its extension is not installed'}</strong>
            {#if title.author || title.artist}
              <span>{[...new Set([title.author, title.artist].filter(Boolean))].join(' · ')}</span>
            {/if}
          </div>
        </div>

        <div class="stats">
          <div><span class="display">{chapters.length}</span><span>Chapters</span></div>
          <div class:hot={unread > 0}><span class="display">{unread}</span><span>Unread</span></div>
          <div><span class="display">{saved}</span><span>Saved</span></div>
        </div>

        {#if panel === 'edit'}
          <form class="panel editor" onsubmit={saveEdit}>
            <strong class="display">EDIT DETAILS</strong>
            <p class="muted">What you change stays as you wrote it; refreshing from the source leaves it alone.</p>
            <label class="field"><span>Name</span><input bind:value={form.title} required maxlength="300" /></label>
            <label class="field"><span>Author</span><input bind:value={form.author} maxlength="300" /></label>
            <label class="field"><span>Artist</span><input bind:value={form.artist} maxlength="300" /></label>
            <label class="field"><span>Genres, with commas between</span><input bind:value={form.genres} /></label>
            <label class="field">
              <span>Status</span>
              <select bind:value={form.status}>
                {#each STATUS as label, value (value)}<option {value}>{label}</option>{/each}
              </select>
            </label>
            <label class="field"><span>Description</span><textarea rows="6" bind:value={form.description} maxlength="20000"></textarea></label>
            <label class="btn picker">
              <Icon name="upload" size={18} />A cover of your own
              <input type="file" accept="image/*" onchange={(e) => setCover(e.currentTarget.files?.[0])} />
            </label>
            <div class="tools">
              <button class="btn ink">Save</button>
              <button class="btn" type="button" onclick={() => (panel = null)}>Cancel</button>
              {#if title.locked.length}
                <button class="btn" type="button" onclick={unlock} title="Edited: {title.locked.join(', ')}">Follow the source again</button>
              {/if}
            </div>
          </form>
        {:else if panel === 'groups'}
          <div class="panel editor">
            <strong class="display">SCANLATION GROUPS</strong>
            <p class="muted">The groups whose uploads you want first. With one upload per chapter, the others are hidden and reading passes over them.</p>
            {#if merged}
              <p class="muted">Sources come before groups: {title.members.map((m) => m.sourceName ?? m.title).join(', ')}.</p>
            {/if}
            <ul class="rows">
              {#each order as name, at (name)}
                <li>
                  <span class="main"><strong>{at + 1}. {name}</strong></span>
                  <span class="tools">
                    <button class="btn icon" aria-label="Move {name} up" disabled={at === 0} onclick={() => moveGroup(at, -1)}><Icon name="up" /></button>
                    <button class="btn icon" aria-label="Move {name} down" disabled={at === order.length - 1} onclick={() => moveGroup(at, 1)}><Icon name="down" /></button>
                  </span>
                </li>
              {/each}
            </ul>
            <button class="btn" aria-pressed={dedupe} onclick={() => (dedupe = !dedupe)}>
              {#if dedupe}<Icon name="check" size={18} stroke={3} />{/if}One upload per chapter
            </button>
            <div class="tools">
              <button class="btn ink" onclick={saveGroups}>Save</button>
              <button class="btn" onclick={() => (panel = null)}>Cancel</button>
            </div>
          </div>
        {/if}

        {#if merged}
          <section class="panel editor" aria-label="Sources">
            <strong>Read from {title.members.length} sources</strong>
            <ul class="rows">
              {#each title.members as member, at (member.id)}
                <li>
                  <span class="main">
                    <strong>{member.sourceName ?? 'Its extension is not installed'}</strong>
                    <span>{member.title}{member.sourceLang ? ` · ${langName(member.sourceLang)}` : ''}{at === 0 ? ' · this title' : ''}</span>
                  </span>
                  {#if at > 0}<button class="btn" onclick={() => unmerge(member.id)}>Take out</button>{/if}
                </li>
              {/each}
            </ul>
          </section>
        {/if}

        {#if title.description}
          <p class="text" class:clamp={!wholeText}>{title.description}</p>
          {#if title.description.length > 280}
            <button class="btn more" aria-expanded={wholeText} onclick={() => (wholeText = !wholeText)}>{wholeText ? 'Less' : 'More'}</button>
          {/if}
        {/if}
        {#if title.genres.length}
          <div class="tools genres">
            {#each title.genres as genre (genre)}<span class="tag">{genre}</span>{/each}
          </div>
        {/if}
        <Tracking {id} title={title.title} />

        {#if marks.length}
          <section class="panel editor" aria-label="Kept pages">
            <strong>Kept pages</strong>
            <ul class="rows">
              {#each marks as kept (`${kept.chapterId}:${kept.page}`)}
                <li>
                  <a class="main" href="#/read/{kept.chapterId}?page={kept.page}">
                    <strong>{kept.chapterName}, page {kept.page + 1}</strong>
                    {#if kept.note}<span>{kept.note}</span>{/if}
                  </a>
                  <button class="btn icon" aria-label="Let go of page {kept.page + 1} of {kept.chapterName}" onclick={() => unmarkPage(kept)}><Icon name="trash" /></button>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      </div>

      <div class="chapters">
        <div class="section-head">
          <h2 class="display">CHAPTERS</h2>
          <div class="tools">
            <button class="btn" aria-expanded={filters} onclick={() => (filters = !filters)}>
              <Icon name="filter" size={18} />Sort &amp; filter{filtering ? ' · on' : ''}
            </button>
            <button class="btn" aria-pressed={picking} onclick={() => (picking ? stopPicking() : startPicking())}>Select</button>
            <button
              class="btn icon"
              aria-label="Download the unread chapters"
              title="Download the unread chapters"
              disabled={!chapters.some((c) => !c.read && !c.downloaded)}
              onclick={() => download(chapters.filter((c) => !c.read && !c.downloaded))}
            >
              <Icon name="download" />
            </button>
            <button class="btn icon" aria-label="Refresh from the source" title="Refresh the chapter list from the source" disabled={refreshing} onclick={refresh}><Icon name="refresh" /></button>
          </div>
        </div>

        {#if filters}
          <div class="tools" role="group" aria-label="Sort and filter">
            <label class="sorter">
              <span class="sr">Sort by</span>
              <select class="btn" value={view.sort} onchange={(e) => setView({ sort: e.currentTarget.value as Sort })}>
                {#each SORTS as [value, label] (value)}<option {value}>{label}</option>{/each}
              </select>
            </label>
            <button class="btn icon" aria-label={view.down ? 'Sorted downwards. Sort upwards' : 'Sorted upwards. Sort downwards'} title={view.down ? 'Sort upwards' : 'Sort downwards'} onclick={() => setView({ down: !view.down })}>
              <Icon name={view.down ? 'down' : 'up'} />
            </button>
            <label class="sorter">
              <span class="sr">Show</span>
              <select class="btn" bind:value={show}>
                {#each SHOWS as [value, label] (value)}<option {value}>{label}</option>{/each}
              </select>
            </label>
            {#if groups.length > 1}
              <label class="sorter">
                <span class="sr">Translated by</span>
                <select class="btn" value={from} onchange={(e) => (group = e.currentTarget.value)}>
                  <option value="">Every group</option>
                  {#each groups as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
            {/if}
            <button class="btn" aria-pressed={view.volumes} onclick={() => setView({ volumes: !view.volumes })}>
              {#if view.volumes}<Icon name="check" size={18} stroke={3} />{/if}By volume
            </button>
            {#if duplicates}
              <button class="btn" aria-pressed={others} onclick={() => (others = !others)}>
                {#if others}<Icon name="check" size={18} stroke={3} />{/if}Other uploads ({duplicates})
              </button>
            {/if}
          </div>
        {/if}

        <Notice loading={refreshing} error={refreshError} retry={refresh} />
        {#if note}<p role="status"><strong>{note}</strong> <a class="link" href="#/downloads">See downloads</a></p>{/if}
        {#if keeping}<p role="status"><strong>{keeping}</strong></p>{/if}

        <ul class="rows">
          {#each listed as chapter, at (chapter.id)}
            {#if view.volumes && (at === 0 || listed[at - 1].volume !== chapter.volume)}
              <li class="volume" aria-hidden="true"><strong>{volumeName(chapter.volume)}</strong></li>
            {/if}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
            <li
              class:done={chapter.read}
              onclickcapture={(e) => pick(e, chapter)}
              onpointerdown={(e) => pressStart(e, chapter)}
              onpointermove={pressMove}
              onpointerup={pressEnd}
              onpointercancel={pressEnd}
              onpointerleave={pressEnd}
              oncontextmenu={(e) => (held || press) && e.preventDefault()}
            >
              <a class="main" href="#/read/{chapter.id}">
                <strong>
                  {#if chapter.bookmark}<span class="kept"><Icon name="bookmark" size={14} fill /><span class="sr">Bookmarked.</span></span>{/if}
                  {chapter.name}
                </strong>
                <span>{about(chapter)}</span>
              </a>
              {#if isNew(chapter)}
                <span class="tag spot">New</span>
              {:else if !chapter.read && chapter.lastPageRead > 0 && chapter.pageCount}
                <span class="meter" aria-hidden="true"><span style:width="{Math.round(((chapter.lastPageRead + 1) / chapter.pageCount) * 100)}%"></span></span>
              {/if}
              {#if picking}
                {#if picked[chapter.id]}<span class="tag ink">Selected</span>{/if}
              {:else}
                {#if chapter.downloaded}
                  <button class="btn icon" aria-label="Delete the saved file of {chapter.name}" title="Delete the saved file" onclick={() => deleteFiles([chapter])}><Icon name="trash" /></button>
                {:else}
                  <button class="btn icon" aria-label="Download {chapter.name}" title="Download" onclick={() => download([chapter])}><Icon name="download" /></button>
                {/if}
                <!-- What is seldom wanted waits behind one button, so a row keeps to two. -->
                <div class="menu own">
                  <button class="btn icon" aria-label="More for {chapter.name}" title="More" aria-expanded={opened === chapter.id} onclick={() => (opened = opened === chapter.id ? null : chapter.id)}>
                    <Icon name="more" fill />
                  </button>
                  {#if opened === chapter.id}
                    <div class="panel sheet">
                      <button class="btn" onclick={() => mark([chapter], !chapter.read)}>
                        <Icon name="check" size={18} stroke={3} />{chapter.read ? 'Mark unread' : 'Mark read'}
                      </button>
                      <!-- Only what is unread is sent: marking a chapter again would move its read date. -->
                      {#if unreadBefore(chapter).length}
                        <button class="btn" onclick={() => markUpTo(chapter)}>
                          <Icon name="check" size={18} stroke={3} />Mark read up to here
                        </button>
                      {/if}
                      <button class="btn" onclick={() => setBookmark([chapter], !chapter.bookmark)}>
                        <Icon name="bookmark" size={18} fill={chapter.bookmark} />{chapter.bookmark ? 'Take the bookmark off' : 'Bookmark'}
                      </button>
                      {#if canKeep}
                        {#if here.has(chapter.id)}
                          <button class="btn" onclick={() => ((opened = null), forgetOnDevice([chapter]))}>Off this device</button>
                        {:else}
                          <button class="btn" onclick={() => ((opened = null), keepOnDevice([chapter]))}>Keep on this device</button>
                        {/if}
                      {/if}
                      <button class="btn" onclick={() => startPicking(chapter)}>Select</button>
                    </div>
                  {/if}
                </div>
              {/if}
            </li>
          {/each}
        </ul>
        {#if !refreshing && listed.length === 0}
          <Notice>{nothing}</Notice>
        {/if}
      </div>
    </div>

    <div class="dock" class:picking>
      {#if picking}
        <div class="line">
          <strong>{chosen.length} selected</strong>
          <button class="btn" onclick={() => (picked = Object.fromEntries(listed.map((c) => [c.id, true])))}>All shown</button>
          <button class="btn" onclick={() => (picked = Object.fromEntries(listed.map((c) => [c.id, !picked[c.id]])))}>Invert</button>
          <button class="btn icon" aria-label="Stop selecting" title="Stop selecting" onclick={stopPicking}><Icon name="close" /></button>
        </div>
        <!-- Each action sends only the chapters it changes: marking a read chapter again would move its read date. -->
        <div class="line" role="group" aria-label="Selected chapters">
          <button class="btn" disabled={!chosen.some((c) => !c.read)} onclick={() => onPicked((list) => mark(list.filter((c) => !c.read), true))}>
            <Icon name="check" size={18} stroke={3} />Mark read
          </button>
          <button class="btn" disabled={!chosen.some((c) => c.read || c.lastPageRead > 0)} onclick={() => onPicked((list) => mark(list, false))}>Mark unread</button>
          {#if chosen.length === 1 && unreadBefore(chosen[0]).length}
            <button class="btn" onclick={() => onPicked((list) => markUpTo(list[0]))}>Mark read up to here</button>
          {/if}
          <button class="btn" disabled={!chosen.length} onclick={() => onPicked((list) => setBookmark(list, !allKept))}>
            <Icon name="bookmark" size={18} fill={allKept} />{allKept ? 'Take bookmarks off' : 'Bookmark'}
          </button>
          <button class="btn" disabled={!chosen.some((c) => !c.downloaded)} onclick={() => onPicked((list) => download(list.filter((c) => !c.downloaded)))}>
            <Icon name="download" size={18} />Download
          </button>
          <button class="btn" disabled={!chosen.some((c) => c.downloaded)} onclick={deletePicked}>
            <Icon name="trash" size={18} />{sure ? 'Really delete?' : 'Delete saved'}
          </button>
          {#if canKeep}
            <button class="btn" disabled={!chosen.length} onclick={() => onPicked((list) => keepOnDevice([...list].reverse()))}>Keep on this device</button>
          {/if}
        </div>
      {:else if resume}
        <a class="btn big spot" href="#/read/{resume.id}">
          {started ? 'Resume' : 'Start'} {chapterShort(resume)}<Icon name="arrow" size={18} stroke={3} />
        </a>
      {:else if chapters.length}
        <a class="btn big" href="#/read/{chapters[chapters.length - 1].id}">Read again<Icon name="arrow" size={18} stroke={3} /></a>
      {/if}
      {#if title.webUrl}
        <a class="btn big icon" href={title.webUrl} target="_blank" rel="noreferrer noopener" aria-label="Open on the source's site" title="Open on the source's site">
          <Icon name="globe" />
        </a>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page {
    gap: 20px;
    padding-bottom: calc(120px + var(--safe-bottom));
  }

  /* The actions on picked chapters take more of the bottom than the dock's one button. */
  .page.picking {
    padding-bottom: calc(220px + var(--safe-bottom));
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  /* With the menu along the bottom this screen stands alone, and its bar is the top of it: always there, for the way back. */
  :global(:root[data-nav='tabs']) .page {
    padding-top: 0;
  }

  :global(:root[data-nav='tabs']) .bar {
    position: sticky;
    top: var(--safe-top);
    z-index: 5;
    margin: 0 calc(-1 * var(--gutter));
    padding: 10px var(--gutter);
    border-bottom: 3px solid var(--ink);
    background: var(--paper);
  }

  .menu {
    position: relative;
  }

  .sheet {
    position: absolute;
    top: 52px;
    right: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    min-width: 240px;
    max-height: 70vh;
    overflow-y: auto;
  }

  .sheet .btn {
    justify-content: flex-start;
    border-width: 0 0 2px;
  }

  .sheet .btn:last-child {
    border-bottom: 0;
  }

  .link {
    text-decoration: underline;
  }

  .cats {
    display: flex;
    flex-direction: column;
  }

  .cats span {
    padding: 10px 14px 6px;
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 28px 48px;
  }

  @media (min-width: 960px) {
    .columns {
      grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
      align-items: start;
    }
  }

  .about {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .hero {
    display: flex;
    gap: 22px;
    padding: 6px 4px 0;
  }

  .art {
    flex: 0 0 clamp(110px, 30%, 170px);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .facts h1 {
    font-size: clamp(24px, 4.4vw, 40px);
    overflow-wrap: anywhere;
  }

  .facts strong,
  .facts > span {
    font-size: 13px;
    font-weight: 700;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: 3px solid var(--ink);
    background: var(--surface);
  }

  .stats div {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 10px 4px;
    border-right: 3px solid var(--ink);
  }

  .stats div:last-child {
    border-right: 0;
  }

  .stats .hot {
    background: var(--spot);
    color: var(--on-spot);
  }

  .stats .display {
    font-size: 22px;
    line-height: 1.15;
  }

  .stats span:last-child {
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .text {
    font-size: 14px;
    line-height: 1.5;
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .clamp {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 5;
    line-clamp: 5;
  }

  .more {
    align-self: flex-start;
  }

  .genres .tag {
    letter-spacing: 0.06em;
  }

  .chapters {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .chapters h2 {
    font-size: 20px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .done .main {
    color: var(--muted);
  }

  .done .main strong {
    font-weight: 700;
  }

  .meter {
    flex: 0 0 56px;
  }

  /* A long press picks a chapter: the phone's own menu for the link and its text selection stay out of the way. */
  @media (pointer: coarse) {
    .rows li {
      -webkit-touch-callout: none;
      user-select: none;
    }
  }

  .dock.picking {
    flex-direction: column;
    align-items: center;
    gap: 10px;
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }

  .sorter select {
    max-width: 220px;
  }

  .own .sheet {
    min-width: 200px;
  }

  .kept {
    display: inline-flex;
    margin-right: 4px;
    vertical-align: -1px;
  }

  .warn {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
  }

  .editor {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 16px;
  }

  .editor .field {
    flex-direction: column;
    align-items: stretch;
    gap: 6px;
    box-shadow: none;
  }

  .editor .field span {
    font-size: 12px;
    font-weight: 900;
  }

  .editor textarea,
  .editor select {
    font: inherit;
  }

  .picker {
    position: relative;
    align-self: flex-start;
  }

  .picker input {
    position: absolute;
    inset: 0;
    opacity: 0;
    cursor: pointer;
  }

  .rows .volume {
    padding: 14px 4px 6px;
    border: 0;
    background: none;
    box-shadow: none;
    font-size: 12px;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .dock {
    position: fixed;
    right: 0;
    bottom: var(--tabs);
    left: 0;
    z-index: 4;
    display: flex;
    justify-content: center;
    gap: 14px;
    padding: 14px max(var(--gutter), var(--safe-right)) calc(14px + var(--safe-bottom)) max(var(--gutter), var(--safe-left));
    border-top: 3px solid var(--ink);
    background: var(--paper);
  }

  .dock > .btn:first-child {
    flex: 0 1 420px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
