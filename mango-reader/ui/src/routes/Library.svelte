<script lang="ts">
  import { untrack } from 'svelte';
  import { offer, tell } from '../lib/undoing.svelte';
  import { get, message, send, type Category, type Library, type LibraryItem } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import { display } from '../lib/display.svelte';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { pref, prefs, setPref } from '../lib/prefs.svelte';
  import { go, ownScroll, route } from '../lib/router.svelte';
  import TitleCard from '../lib/TitleCard.svelte';
  import { startTour, tour } from '../lib/guide.svelte';

  type Sort = 'lastRead' | 'latest' | 'unread' | 'title' | 'added';
  // `source` is a source's name; '' is every source.
  type View = { layout: 'grid' | 'list'; sort: Sort; down: boolean; unread: boolean; downloaded: boolean; completed: boolean; source: string };

  const SORTS: [Sort, string][] = [
    ['lastRead', 'Last read'],
    ['latest', 'Last updated'],
    ['unread', 'Unread count'],
    ['title', 'Title'],
    ['added', 'Date added'],
  ];
  const KEYS: Record<Sort, (item: LibraryItem) => number | string> = {
    lastRead: (item) => item.lastReadAt,
    latest: (item) => item.latestAt,
    unread: (item) => item.unread,
    title: (item) => item.title.toLowerCase(),
    added: (item) => item.addedAt,
  };
  const DEFAULT: View = { layout: 'grid', sort: 'lastRead', down: true, unread: false, downloaded: false, completed: false, source: '' };
  /** What a title is called when its extension is not installed, in the source filter. */
  const NO_SOURCE = 'Not installed';

  // What was here last time shows at once; the library is then asked for again.
  const kept = recall<Library>('library');
  let data = $state<Library | null>(kept);
  let loading = $state(!kept);
  let error = $state<string | null>(null);
  // Where the shelf was left, held for as long as the page lives: coming back from a title
  // finds the same category, search and page, and the router then puts the scroll back.
  // A title page's author and genre links open the library searched for them, from the top.
  type Place = { category: number | null; text: string; page: number };
  const asked = route.query.get('q');
  const left = asked === null ? recall<Place>('library.place') : null;
  // null is "All".
  let category = $state<number | null>(left?.category ?? null);
  let text = $state(asked ?? left?.text ?? '');
  let picking = $state(false);
  let picked = $state<Record<number, boolean>>({});
  let filters = $state(false);
  let sure = $state(false);
  // With the library a page at a time, the page on show, from 0.
  let page = $state(left?.page ?? 0);

  $effect(() => keep('library.place', { category, text, page } satisfies Place));

  // The title opened from the shelf: its place in the list and how far down the screen its card
  // was. Cards off screen are not laid out until they come near (lib/TitleCard.svelte), so on the
  // way back the rows above are only guessed at and the scroll the router keeps lands rows away.
  // The card is put back where it was instead, once, and only on the way back.
  type Spot = { id: number; at: number; top: number };
  const spot = route.returned ? recall<Spot>('library.spot') : null;
  keep('library.spot', null);
  let shelf = $state<HTMLElement>();

  // A new account lands here with an empty library: the tour starts by itself, once.
  // An account that has titles already found its way, and is not shown it unasked.
  $effect(() => {
    if (data && data.items.length === 0 && prefs.ready && !tour.open && !pref('tour.done', false)) startTour();
  });

  const view = $derived<View>({ ...DEFAULT, ...pref<Partial<View>>('library.view', {}) });

  /** The categories in their tree: each one after the one it sits inside, with how deep it is. */
  const tree = $derived.by(() => {
    const all = data?.categories ?? [];
    const out: { category: Category; depth: number }[] = [];
    const known = new Set(all.map((c) => c.id));
    const place = (parent: number | null, depth: number) => {
      for (const category of all.filter((c) => (c.parent !== null && known.has(c.parent) ? c.parent : null) === parent)) {
        if (out.some((entry) => entry.category.id === category.id)) continue;
        out.push({ category, depth });
        place(category.id, depth + 1);
      }
    };
    place(null, 0);
    return out;
  });

  /** A category and every category inside it, however deep: showing one shows what is inside. */
  const within = $derived.by(() => {
    if (category === null) return null;
    const ids = new Set([category]);
    for (let grown = true; grown; ) {
      grown = false;
      for (const c of data?.categories ?? []) {
        if (c.parent !== null && ids.has(c.parent) && !ids.has(c.id)) {
          ids.add(c.id);
          grown = true;
        }
      }
    }
    return ids;
  });

  /** The sources the library's titles come from, by name, for the source filter. */
  const sources = $derived([...new Set((data?.items ?? []).map((item) => item.sourceName ?? NO_SOURCE))].sort((a, b) => a.localeCompare(b)));
  // A source picked once and since gone from the library filters nothing.
  const source = $derived(sources.includes(view.source) ? view.source : '');

  /** The search finds a title by its name, who made it, or a genre. */
  const matches = (item: LibraryItem, needle: string) =>
    [item.title, item.author, item.artist, item.genre].some((field) => field?.toLowerCase().includes(needle));

  const shown = $derived.by(() => {
    if (!data) return [];
    const needle = text.trim().toLowerCase();
    const list = data.items.filter(
      (item) =>
        (within === null || item.categories.some((id) => within.has(id))) &&
        (!needle || matches(item, needle)) &&
        (!source || (item.sourceName ?? NO_SOURCE) === source) &&
        (!view.unread || item.unread > 0) &&
        (!view.downloaded || item.downloaded > 0) &&
        (!view.completed || item.status === 2 || item.status === 4),
    );
    const key = KEYS[view.sort];
    const turn = view.down ? -1 : 1;
    return list.sort((a, b) => {
      const [x, y] = [key(a), key(b)];
      return x === y ? a.title.localeCompare(b.title) : (x < y ? -1 : 1) * turn;
    });
  });
  const chosen = $derived(Object.keys(picked).filter((id) => picked[Number(id)]).map(Number));
  const filtering = $derived(view.unread || view.downloaded || view.completed || !!source);
  const size = $derived(display.value.pages);
  const pages = $derived(size ? Math.max(1, Math.ceil(shown.length / size)) : 1);
  const onPage = $derived(size ? shown.slice(Math.min(page, pages - 1) * size, (Math.min(page, pages - 1) + 1) * size) : shown);

  // Narrowing the list starts it from its first page; opening the screen keeps the page it was left on.
  let opened = false;
  $effect(() => {
    void [category, text, view];
    if (opened) page = 0;
    opened = true;
  });

  // A title read meanwhile may have moved in the list ("Last read"): then the router's scroll stays.
  $effect(() => {
    if (!spot || !shelf || untrack(() => onPage.findIndex((item) => item.id === spot.id)) !== spot.at) return;
    ownScroll();
    return putBack(shelf, spot);
  });

  /** Scrolls until the card is where it was, as the cards around it are laid out; a touch, the wheel or a key stops it. */
  function putBack(shelf: HTMLElement, spot: Spot) {
    let frame = 0;
    let still = 0;
    const until = performance.now() + 1500;
    const stop = () => cancelAnimationFrame(frame);
    const step = () => {
      const card = shelf.querySelector(`a[href="#/title/${spot.id}"]`);
      if (!card) return;
      const off = card.getBoundingClientRect().top - spot.top;
      if (Math.abs(off) > 1) {
        scrollBy(0, off);
        still = 0;
      } else still++;
      if (still < 3 && performance.now() < until) frame = requestAnimationFrame(step);
    };
    step();
    const kinds = ['touchstart', 'wheel', 'keydown'];
    for (const kind of kinds) addEventListener(kind, stop, { passive: true });
    return () => {
      stop();
      for (const kind of kinds) removeEventListener(kind, stop);
    };
  }

  /** Notes the title opened from the shelf, for the way back. */
  function leaving(event: MouseEvent) {
    if (picking || event.defaultPrevented || event.button !== 0 || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
    const link = (event.target as Element).closest('a[href^="#/title/"]');
    const id = Number(link?.getAttribute('href')?.slice('#/title/'.length));
    const at = onPage.findIndex((item) => item.id === id);
    if (link && at >= 0) keep('library.spot', { id, at, top: link.getBoundingClientRect().top } satisfies Spot);
  }

  function turn(to: number) {
    page = Math.max(0, Math.min(pages - 1, to));
    scrollTo({ top: 0 });
  }

  async function load() {
    error = null;
    try {
      const fresh = await get<Library>('/library');
      keep('library', fresh);
      data = fresh;
      if (category !== null && !data.categories.some((c) => c.id === category)) category = null;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  function setView(patch: Partial<View>) {
    setPref('library.view', { ...view, ...patch }).catch((e) => (error = message(e)));
  }

  function setSort(sort: Sort) {
    // Names read best from A; everything else from the newest or the most.
    setView({ sort, down: sort !== 'title' });
  }

  function stopPicking() {
    picking = false;
    picked = {};
    sure = false;
  }

  async function act(body: Record<string, unknown>) {
    error = null;
    const count = chosen.length;
    try {
      await send('POST', '/library/bulk', { ids: chosen, ...body });
      stopPicking();
      if (body.action === 'download') tell(`Unread chapters of ${many(count, 'title')} queued for download.`);
      else if (body.action === 'categories') tell(`${many(count, 'title')} put in the category.`);
      await load();
    } catch (e) {
      error = message(e);
    }
  }

  const many = (n: number, one: string) => `${n} ${one}${n === 1 ? '' : 's'}`;

  /**
   * Taking titles out and marking them read or unread are held back for Undo (lib/undoing.svelte.ts):
   * the shelf shows the change at once and Mango is told a few seconds later.
   */
  function hold(action: 'remove' | 'read' | 'unread') {
    if (!data || !chosen.length) return;
    error = null;
    const ids = chosen;
    const before = $state.snapshot(data);
    if (action === 'remove') data.items = data.items.filter((item) => !ids.includes(item.id));
    else for (const item of data.items) if (ids.includes(item.id)) item.unread = action === 'read' ? 0 : item.total;
    stopPicking();
    const text = { remove: 'taken out of the library', read: 'marked read', unread: 'marked unread' }[action];
    offer(`${many(ids.length, 'title')} ${text}.`, {
      undo: () => (data = before),
      commit: () => send('POST', '/library/bulk', { ids, action }),
      then: load,
      failed: load,
    });
  }

  function remove() {
    if (sure) hold('remove');
    else sure = true;
  }

  /** While picking, a click on a title selects it instead of opening it. */
  function pick(event: MouseEvent, id: number) {
    if (!picking) return;
    event.preventDefault();
    event.stopPropagation();
    picked[id] = !picked[id];
    sure = false;
  }
</script>

<div class="page">
  <div class="page-head">
    <div class="heading">
      <h1 class="display caps">Library</h1>
      {#if data}
        <strong>{data.items.length} {data.items.length === 1 ? 'title' : 'titles'} · {data.categories.length} {data.categories.length === 1 ? 'category' : 'categories'}</strong>
      {/if}
      <a class="btn" href="#/stats">Stats</a>
    </div>
    {#if data?.categories.length}
      <div class="tools slide" role="group" aria-label="Category">
        <button class="btn pill" aria-pressed={category === null} onclick={() => (category = null)}>All</button>
        {#each tree as { category: c, depth } (c.id)}
          <button class="btn pill" aria-pressed={category === c.id} onclick={() => (category = c.id)}>{'› '.repeat(depth)}{c.name}</button>
        {/each}
      </div>
    {/if}
  </div>

  <Notice {loading} {error} retry={load} />

  {#if data}
    {#if data.resume}
      {@const r = data.resume}
      <section class="panel resume" aria-label="Continue reading">
        <span class="corner" aria-hidden="true"></span>
        <div class="art"><Cover src={r.cover} title={r.title} tilt /></div>
        <div class="words">
          <span class="tag ink">Continue reading</span>
          <h2 class="display">{r.title}</h2>
          <p>{r.chapterName}</p>
          {#if r.pageCount}
            <div class="progress">
              <span class="meter"><span style:width="{Math.round(((r.page + 1) / r.pageCount) * 100)}%"></span></span>
              <strong>{r.page + 1} / {r.pageCount}</strong>
            </div>
          {/if}
          <div class="tools">
            <a class="btn big spot" href="#/read/{r.chapterId}">Resume<Icon name="arrow" size={18} stroke={3} /></a>
            <a class="btn big" href="#/title/{r.mangaId}">Chapters</a>
          </div>
        </div>
      </section>
    {/if}

    {#if data.items.length === 0}
      <Notice>
        Nothing in the library yet. Find a title and add it.
        <a class="btn spot" href="#/browse">Browse</a>
      </Notice>
    {:else}
      <!-- Only notes which title a link opened; the links do the opening. -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <section class="shelf" bind:this={shelf} onclick={leaving}>
        <div class="section-head">
          <h2 class="display" class:caps={category === null}>{category === null ? 'All titles' : data.categories.find((c) => c.id === category)?.name}</h2>
          <div class="tools">
            <label class="field find">
              <span class="sr">Search the library</span>
              <Icon name="search" size={18} stroke={3} />
              <input type="search" bind:value={text} placeholder="Search the library" />
            </label>
            <!-- On a narrow screen the order and the layout wait behind the filter button, to keep this to one line. -->
            <div class="view" class:open={filters}>
              <label class="sorter">
                <span class="sr">Sort by</span>
                <select class="btn" value={view.sort} onchange={(e) => setSort(e.currentTarget.value as Sort)}>
                  {#each SORTS as [value, label] (value)}<option {value}>{label}</option>{/each}
                </select>
              </label>
              <button class="btn icon" aria-label={view.down ? 'Sorted downwards. Sort upwards' : 'Sorted upwards. Sort downwards'} onclick={() => setView({ down: !view.down })}>
                <Icon name={view.down ? 'down' : 'up'} />
              </button>
              <button class="btn icon late" aria-label="Grid view" aria-pressed={view.layout === 'grid'} onclick={() => setView({ layout: 'grid' })}><Icon name="grid" size={18} /></button>
              <button class="btn icon late" aria-label="List view" aria-pressed={view.layout === 'list'} onclick={() => setView({ layout: 'list' })}><Icon name="list" size={18} /></button>
            </div>
            <button class="btn" aria-expanded={filters} onclick={() => (filters = !filters)}>
              <Icon name="filter" size={18} />Filter{filtering ? ' on' : ''}
            </button>
            <button class="btn last" aria-pressed={picking} onclick={() => (picking ? stopPicking() : (picking = true))}>Select</button>
          </div>
        </div>

        {#if filters}
          <div class="tools" role="group" aria-label="Show only">
            <button class="btn pill" aria-pressed={view.unread} onclick={() => setView({ unread: !view.unread })}>Unread</button>
            <button class="btn pill" aria-pressed={view.downloaded} onclick={() => setView({ downloaded: !view.downloaded })}>Downloaded</button>
            <button class="btn pill" aria-pressed={view.completed} onclick={() => setView({ completed: !view.completed })}>Completed</button>
            {#if sources.length > 1}
              <label>
                <span class="sr">Source</span>
                <select class="btn" value={source} onchange={(e) => setView({ source: e.currentTarget.value })}>
                  <option value="">All sources</option>
                  {#each sources as name (name)}<option value={name}>{name}</option>{/each}
                </select>
              </label>
            {/if}
          </div>
        {/if}

        {#if picking}
          <div class="panel bulk" role="group" aria-label="Selected titles">
            <strong>{chosen.length} selected</strong>
            <button class="btn" onclick={() => (picked = Object.fromEntries(shown.map((item) => [item.id, true])))}>All shown</button>
            <button class="btn" disabled={!chosen.length} onclick={() => hold('read')}>Mark read</button>
            <button class="btn" disabled={!chosen.length} onclick={() => hold('unread')}>Mark unread</button>
            <button class="btn" disabled={!chosen.length} onclick={() => act({ action: 'download' })}><Icon name="download" size={18} />Download unread</button>
            <button class="btn" disabled={!chosen.length} onclick={() => go(`/migrate/many?ids=${chosen.join(',')}`)}><Icon name="arrow" size={18} />Move to other sources</button>
            {#if data.categories.length}
              <label>
                <span class="sr">Add to a category</span>
                <select class="btn" disabled={!chosen.length} onchange={(e) => e.currentTarget.value && act({ action: 'categories', categories: [Number(e.currentTarget.value)] })}>
                  <option value="">Add to category…</option>
                  {#each tree as { category: c, depth } (c.id)}<option value={c.id}>{'› '.repeat(depth)}{c.name}</option>{/each}
                </select>
              </label>
            {/if}
            <button class="btn" disabled={!chosen.length} onclick={remove}><Icon name="trash" size={18} />{sure ? 'Really remove?' : 'Remove'}</button>
            <button class="btn icon" aria-label="Stop selecting" onclick={stopPicking}><Icon name="close" /></button>
          </div>
        {/if}

        {#if shown.length === 0}
          <Notice>No title matches.</Notice>
        {:else if view.layout === 'grid'}
          <div class="grid">
            {#each onPage as item (item.id)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <div class="slot" class:picked={picked[item.id]} onclickcapture={(e) => pick(e, item.id)}>
                <TitleCard id={item.id} title={item.title} cover={item.cover} unread={item.unread}>
                  {#snippet meta()}
                    <span>{item.total - item.unread} / {item.total}</span><span>{item.sourceName ?? ''}</span>
                  {/snippet}
                </TitleCard>
                {#if picked[item.id]}<span class="tick"><Icon name="check" stroke={4} /><span class="sr">Selected</span></span>{/if}
              </div>
            {/each}
          </div>
        {:else}
          <ul class="rows">
            {#each onPage as item (item.id)}
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
              <li class:picked={picked[item.id]} onclickcapture={(e) => pick(e, item.id)}>
                <a class="main" href="#/title/{item.id}">
                  <strong>{item.title}</strong>
                  <span>{item.total - item.unread} / {item.total}{item.sourceName ? ` · ${item.sourceName}` : ''}{item.downloaded ? ` · ${item.downloaded} saved` : ''}</span>
                </a>
                {#if picked[item.id]}<span class="tag ink">Selected</span>{/if}
                {#if item.unread > 0}<span class="badge"><span class="sr">Unread chapters: </span>{item.unread}</span>{/if}
              </li>
            {/each}
          </ul>
        {/if}
        {#if pages > 1}
          <nav class="pager" aria-label="Pages of the library">
            <button class="btn" disabled={page === 0} onclick={() => turn(page - 1)}><Icon name="back" size={18} stroke={3} />Before</button>
            <strong>Page {Math.min(page, pages - 1) + 1} of {pages}</strong>
            <button class="btn" disabled={page >= pages - 1} onclick={() => turn(page + 1)}>After<Icon name="forward" size={18} stroke={3} /></button>
          </nav>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  .heading {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 6px 18px;
  }

  .resume {
    position: relative;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 28px;
    overflow: hidden;
    padding: 28px;
  }

  .corner {
    position: absolute;
    top: 0;
    right: 0;
    width: 220px;
    height: 220px;
    background-image: var(--dots);
    background-size: 9px 9px;
    clip-path: polygon(100% 0, 0 0, 100% 100%);
  }

  .art {
    position: relative;
    flex: 0 0 clamp(96px, 22vw, 150px);
  }

  .words {
    position: relative;
    display: flex;
    flex: 1 1 280px;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    min-width: 0;
  }

  .words h2 {
    font-size: clamp(24px, 4.5vw, 38px);
    overflow-wrap: anywhere;
  }

  .words p {
    font-size: 16px;
    font-weight: 700;
  }

  .progress {
    display: flex;
    align-items: center;
    gap: 12px;
    width: min(420px, 100%);
  }

  .progress .meter {
    flex: 1;
    height: 16px;
    border-width: 3px;
  }

  .progress .meter span {
    border-right: 3px solid var(--ink);
    background: var(--spot);
  }

  .progress strong {
    font-size: 13px;
    font-weight: 900;
  }

  .shelf {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .find {
    flex: 1 1 200px;
    box-shadow: none;
  }

  /* Beside each other on a wide screen, in the order they have always had. */
  .view {
    display: contents;
  }

  .late {
    order: 1;
  }

  .last {
    order: 2;
  }

  @media (max-width: 640px) {
    .section-head .tools {
      flex: 1 1 100%;
    }

    .find {
      flex-basis: 80px;
    }

    .view {
      display: none;
    }

    .view.open {
      display: flex;
      flex-basis: 100%;
      gap: 8px;
      order: 3;
    }

    .sorter {
      flex: 1;
    }

    .sorter select {
      width: 100%;
    }

    .late,
    .last {
      order: 0;
    }

    .resume {
      gap: 16px;
      padding: 16px;
    }

    .corner {
      width: 120px;
      height: 120px;
    }

    .art {
      flex-basis: 96px;
    }

    .words {
      flex-basis: 170px;
      gap: 10px;
    }

    .words .tools {
      align-self: stretch;
    }

    .words .btn {
      flex: 1;
    }
  }

  /* Stays in reach while titles far down the shelf are picked. */
  .bulk {
    position: sticky;
    top: calc(var(--safe-top) + 8px);
    z-index: 3;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 12px;
    box-shadow: var(--sh-4);
  }

  .bulk strong {
    margin-right: 6px;
  }

  .slot {
    position: relative;
  }

  .slot.picked {
    outline: 4px solid var(--ink);
    outline-offset: 6px;
  }

  .tick {
    position: absolute;
    top: 6px;
    left: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 40px;
    height: 40px;
    border: 3px solid var(--ink);
    background: var(--spot);
    color: var(--on-spot);
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .pager {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 16px;
  }
</style>
