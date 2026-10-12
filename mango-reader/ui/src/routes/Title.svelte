<script lang="ts">
  import { get, message, send, type Category, type Chapter, type Title } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { pref, setPref } from '../lib/prefs.svelte';
  import { back } from '../lib/router.svelte';
  import Tracking from '../lib/Tracking.svelte';
  import { ago, chapterShort, langName, STATUS } from '../lib/util';

  const WEEK = 7 * 86400;

  type Sort = 'source' | 'number' | 'date';
  type View = { sort: Sort; down: boolean };

  const SORTS: [Sort, string][] = [
    ['source', 'Source order'],
    ['number', 'Number'],
    ['date', 'Date'],
  ];
  const DEFAULT: View = { sort: 'source', down: true };

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
  // What the list is narrowed to lasts as long as the page: the next title opened shows whole.
  let only = $state({ unread: false, downloaded: false, bookmarked: false });
  let group = $state('');

  const chapters = $derived(title?.chapters ?? []);
  const unread = $derived(chapters.filter((c) => !c.read).length);
  const saved = $derived(chapters.filter((c) => c.downloaded).length);
  // The order is kept for every title, on the server like the library's.
  const view = $derived<View>({ ...DEFAULT, ...pref<Partial<View>>('title.view', {}) });
  const groups = $derived([...new Set(chapters.map((c) => c.scanlator ?? '').filter(Boolean))].sort((a, b) => a.localeCompare(b)));
  // A group a refresh took away narrows nothing.
  const from = $derived(groups.includes(group) ? group : '');
  const filtering = $derived(only.unread || only.downloaded || only.bookmarked || from !== '');
  const listed = $derived.by(() => {
    const list = chapters.filter(
      (c) => (!only.unread || !c.read) && (!only.downloaded || c.downloaded) && (!only.bookmarked || c.bookmark) && (!from || c.scanlator === from),
    );
    // Sources list the newest first, and so does every order here until it is turned round.
    if (view.sort !== 'source') {
      const key = view.sort === 'number' ? (c: Chapter) => c.number : (c: Chapter) => c.dateUpload || c.fetchedAt * 1000;
      list.sort((a, b) => key(b) - key(a));
    }
    return view.down ? list : list.reverse();
  });
  const nothing = $derived(
    !chapters.length ? 'No chapters yet.' : only.unread && !only.downloaded && !only.bookmarked && !from ? 'Every chapter is read.' : 'No chapter fits the filter.',
  );
  // Reading goes from the bottom of the list up: the next one to read is the last unread.
  const resume = $derived(chapters.findLast((c) => !c.read) ?? null);
  const started = $derived(chapters.some((c) => c.read || c.lastPageRead > 0));

  async function load() {
    loading = !title;
    error = null;
    try {
      const fresh = await get<Title>(`/manga/${id}`);
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

  function deleteFile(chapter: Chapter) {
    chapter.downloaded = false;
    change(() => send('DELETE', `/chapters/${chapter.id}/download`));
  }

  function toggleAuto() {
    if (!title) return;
    const t = title;
    t.autoDownload = !t.autoDownload;
    change(() => send('PUT', `/manga/${id}/auto-download`, { on: t.autoDownload }), true);
  }

  async function change(work: () => Promise<unknown>, keepMenu = false) {
    menu = keepMenu && menu;
    refreshError = null;
    try {
      await work();
    } catch (e) {
      refreshError = message(e);
      title = await get<Title>(`/manga/${id}`).catch(() => title);
    }
  }

  function toggleLibrary() {
    if (!title) return;
    const t = title;
    t.inLibrary = !t.inLibrary;
    change(() => send('PUT', `/manga/${id}/library`, { inLibrary: t.inLibrary }));
  }

  function mark(list: Chapter[], read: boolean) {
    for (const chapter of list) {
      chapter.read = read;
      chapter.lastPageRead = 0;
    }
    change(() => send('PUT', `/manga/${id}/chapters`, { ids: list.map((c) => c.id), read }));
  }

  function bookmark(chapter: Chapter) {
    chapter.bookmark = !chapter.bookmark;
    change(() => send('PUT', `/manga/${id}/chapters`, { ids: [chapter.id], bookmark: chapter.bookmark }));
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
    return [when, chapter.scanlator, where, chapter.downloaded ? 'saved' : ''].filter(Boolean).join(' · ');
  }

  function isNew(chapter: Chapter): boolean {
    return !chapter.read && chapter.lastPageRead === 0 && !!title?.addedAt && chapter.fetchedAt > title.addedAt && Date.now() / 1000 - chapter.fetchedAt < WEEK;
  }
</script>

<div class="page">
  <Notice {loading} {error} retry={load} />

  {#if title}
    <div class="bar">
      <a class="btn icon lift" href="#/" aria-label="Back" onclick={(e) => (e.preventDefault(), back())}>
        <Icon name="back" stroke={3} />
      </a>
      <div class="tools">
        <button
          class="btn lift spot-on"
          aria-pressed={title.inLibrary}
          onclick={toggleLibrary}
        >
          <Icon name="bookmark" fill={title.inLibrary} />{title.inLibrary ? 'In library' : 'Add to library'}
        </button>
        <div class="menu">
          <button class="btn icon lift" aria-label="More actions" aria-expanded={menu} onclick={() => (menu = !menu)}><Icon name="more" fill /></button>
          {#if menu}
            <div class="panel sheet">
              <button class="btn" onclick={refresh}><Icon name="refresh" size={18} />Refresh from the source</button>
              <button class="btn" onclick={() => mark(chapters, true)}><Icon name="check" size={18} />Mark all read</button>
              <button class="btn" onclick={() => mark(chapters, false)}>Mark all unread</button>
              <a class="btn" href="#/search?q={encodeURIComponent(title.title)}&for={id}"><Icon name="arrow" size={18} />Move to another source</a>
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
      </div>

      <div class="chapters">
        <div class="section-head">
          <h2 class="display">CHAPTERS</h2>
          <div class="tools">
            <!-- On a narrow screen the order waits behind the filter button, to keep this to one line. -->
            <div class="view" class:open={filters}>
              <label class="sorter">
                <span class="sr">Sort by</span>
                <select class="btn" value={view.sort} onchange={(e) => setView({ sort: e.currentTarget.value as Sort })}>
                  {#each SORTS as [value, label] (value)}<option {value}>{label}</option>{/each}
                </select>
              </label>
              <button class="btn icon" aria-label={view.down ? 'Sorted downwards. Sort upwards' : 'Sorted upwards. Sort downwards'} onclick={() => setView({ down: !view.down })}>
                <Icon name={view.down ? 'down' : 'up'} />
              </button>
            </div>
            <button class="btn" aria-expanded={filters} onclick={() => (filters = !filters)}><Icon name="filter" size={18} />Filter{filtering ? ' on' : ''}</button>
            <button
              class="btn icon"
              aria-label="Download the unread chapters"
              disabled={!chapters.some((c) => !c.read && !c.downloaded)}
              onclick={() => download(chapters.filter((c) => !c.read && !c.downloaded))}
            >
              <Icon name="download" />
            </button>
            <button class="btn icon" aria-label="Refresh from the source" disabled={refreshing} onclick={refresh}><Icon name="refresh" /></button>
          </div>
        </div>

        {#if filters}
          <div class="tools" role="group" aria-label="Show only">
            <button class="btn pill" aria-pressed={only.unread} onclick={() => (only.unread = !only.unread)}>Unread</button>
            <button class="btn pill" aria-pressed={only.downloaded} onclick={() => (only.downloaded = !only.downloaded)}>Downloaded</button>
            <button class="btn pill" aria-pressed={only.bookmarked} onclick={() => (only.bookmarked = !only.bookmarked)}>Bookmarked</button>
            {#if groups.length > 1}
              <label class="sorter">
                <span class="sr">Translated by</span>
                <select class="btn" value={from} onchange={(e) => (group = e.currentTarget.value)}>
                  <option value="">Every group</option>
                  {#each groups as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
            {/if}
          </div>
        {/if}

        <Notice loading={refreshing} error={refreshError} retry={refresh} />
        {#if note}<p role="status"><strong>{note}</strong> <a class="link" href="#/downloads">See downloads</a></p>{/if}

        <ul class="rows">
          {#each listed as chapter (chapter.id)}
            <li class:done={chapter.read}>
              <a class="main" href="#/read/{chapter.id}">
                <strong>{chapter.name}</strong>
                <span>{about(chapter)}</span>
              </a>
              {#if isNew(chapter)}
                <span class="tag spot">New</span>
              {:else if !chapter.read && chapter.lastPageRead > 0 && chapter.pageCount}
                <span class="meter" aria-hidden="true"><span style:width="{Math.round(((chapter.lastPageRead + 1) / chapter.pageCount) * 100)}%"></span></span>
              {/if}
              <button
                class="btn icon"
                aria-pressed={chapter.bookmark}
                aria-label={chapter.bookmark ? `Take the bookmark off ${chapter.name}` : `Bookmark ${chapter.name}`}
                onclick={() => bookmark(chapter)}
              >
                <Icon name="bookmark" fill={chapter.bookmark} />
              </button>
              {#if chapter.downloaded}
                <button class="btn icon" aria-label="Delete the saved file of {chapter.name}" onclick={() => deleteFile(chapter)}><Icon name="trash" /></button>
              {:else}
                <button class="btn icon" aria-label="Download {chapter.name}" onclick={() => download([chapter])}><Icon name="download" /></button>
              {/if}
              <button
                class="btn icon"
                aria-pressed={chapter.read}
                aria-label={chapter.read ? `Mark ${chapter.name} unread` : `Mark ${chapter.name} read`}
                onclick={() => mark([chapter], !chapter.read)}
              >
                <Icon name="check" stroke={3} />
              </button>
            </li>
          {/each}
        </ul>
        {#if !refreshing && listed.length === 0}
          <Notice>{nothing}</Notice>
        {/if}
      </div>
    </div>

    <div class="dock">
      {#if resume}
        <a class="btn big spot" href="#/read/{resume.id}">
          {started ? 'Resume' : 'Start'} {chapterShort(resume)}<Icon name="arrow" size={18} stroke={3} />
        </a>
      {:else if chapters.length}
        <a class="btn big" href="#/read/{chapters[chapters.length - 1].id}">Read again<Icon name="arrow" size={18} stroke={3} /></a>
      {/if}
      {#if title.webUrl}
        <a class="btn big icon" href={title.webUrl} target="_blank" rel="noreferrer noopener" aria-label="Open on the source's site">
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

  /* Beside the other tools on a wide screen. */
  .view {
    display: contents;
  }

  .sorter select {
    max-width: 220px;
  }

  @media (max-width: 640px) {
    .section-head .tools {
      flex: 1 1 100%;
      justify-content: flex-end;
    }

    .view {
      display: none;
    }

    .view.open {
      display: flex;
      flex-basis: 100%;
      gap: 8px;
      order: 1;
    }

    .view .sorter {
      flex: 1;
    }

    .view .sorter select {
      width: 100%;
      max-width: none;
    }

    /* Three buttons to a row leave the name little room: the words under it say the page already. */
    .meter {
      display: none;
    }

    .rows > li {
      gap: 6px;
    }
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

  .dock .btn:first-child {
    flex: 0 1 420px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
