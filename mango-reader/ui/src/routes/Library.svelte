<script lang="ts">
  import { get, message, send, type Library, type LibraryItem } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { pref, setPref } from '../lib/prefs.svelte';
  import TitleCard from '../lib/TitleCard.svelte';

  type Sort = 'lastRead' | 'latest' | 'unread' | 'title' | 'added';
  type View = { layout: 'grid' | 'list'; sort: Sort; down: boolean; unread: boolean; downloaded: boolean; completed: boolean };

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
  const DEFAULT: View = { layout: 'grid', sort: 'lastRead', down: true, unread: false, downloaded: false, completed: false };

  let data = $state<Library | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  // null is "All".
  let category = $state<number | null>(null);
  let text = $state('');
  let picking = $state(false);
  let picked = $state<Record<number, boolean>>({});
  let filters = $state(false);
  let sure = $state(false);

  const view = $derived<View>({ ...DEFAULT, ...pref<Partial<View>>('library.view', {}) });
  const shown = $derived.by(() => {
    if (!data) return [];
    const needle = text.trim().toLowerCase();
    const list = data.items.filter(
      (item) =>
        (category === null || item.categories.includes(category)) &&
        (!needle || item.title.toLowerCase().includes(needle)) &&
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
  const filtering = $derived(view.unread || view.downloaded || view.completed);

  async function load() {
    error = null;
    try {
      data = await get<Library>('/library');
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
    try {
      await send('POST', '/library/bulk', { ids: chosen, ...body });
      stopPicking();
      await load();
    } catch (e) {
      error = message(e);
    }
  }

  function remove() {
    if (sure) act({ action: 'remove' });
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
      <h1 class="display">LIBRARY</h1>
      {#if data}
        <strong>{data.items.length} {data.items.length === 1 ? 'title' : 'titles'} · {data.categories.length} {data.categories.length === 1 ? 'category' : 'categories'}</strong>
      {/if}
    </div>
    {#if data?.categories.length}
      <div class="tools" role="group" aria-label="Category">
        <button class="btn pill" aria-pressed={category === null} onclick={() => (category = null)}>All</button>
        {#each data.categories as c (c.id)}
          <button class="btn pill" aria-pressed={category === c.id} onclick={() => (category = c.id)}>{c.name}</button>
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
      <section class="shelf">
        <div class="section-head">
          <h2 class="display">{category === null ? 'ALL TITLES' : data.categories.find((c) => c.id === category)?.name}</h2>
          <div class="tools">
            <label class="field find">
              <span class="sr">Search the library</span>
              <Icon name="search" size={18} stroke={3} />
              <input type="search" bind:value={text} placeholder="Search the library" />
            </label>
            <label class="sorter">
              <span class="sr">Sort by</span>
              <select class="btn" value={view.sort} onchange={(e) => setSort(e.currentTarget.value as Sort)}>
                {#each SORTS as [value, label] (value)}<option {value}>{label}</option>{/each}
              </select>
            </label>
            <button class="btn icon" aria-label={view.down ? 'Sorted downwards. Sort upwards' : 'Sorted upwards. Sort downwards'} onclick={() => setView({ down: !view.down })}>
              <Icon name={view.down ? 'down' : 'up'} />
            </button>
            <button class="btn" aria-expanded={filters} onclick={() => (filters = !filters)}>
              <Icon name="filter" size={18} />Filter{filtering ? ' on' : ''}
            </button>
            <button class="btn icon" aria-label="Grid view" aria-pressed={view.layout === 'grid'} onclick={() => setView({ layout: 'grid' })}><Icon name="grid" size={18} /></button>
            <button class="btn icon" aria-label="List view" aria-pressed={view.layout === 'list'} onclick={() => setView({ layout: 'list' })}><Icon name="list" size={18} /></button>
            <button class="btn" aria-pressed={picking} onclick={() => (picking ? stopPicking() : (picking = true))}>Select</button>
          </div>
        </div>

        {#if filters}
          <div class="tools" role="group" aria-label="Show only">
            <button class="btn pill" aria-pressed={view.unread} onclick={() => setView({ unread: !view.unread })}>Unread</button>
            <button class="btn pill" aria-pressed={view.downloaded} onclick={() => setView({ downloaded: !view.downloaded })}>Downloaded</button>
            <button class="btn pill" aria-pressed={view.completed} onclick={() => setView({ completed: !view.completed })}>Completed</button>
          </div>
        {/if}

        {#if picking}
          <div class="panel bulk" role="group" aria-label="Selected titles">
            <strong>{chosen.length} selected</strong>
            <button class="btn" onclick={() => (picked = Object.fromEntries(shown.map((item) => [item.id, true])))}>All shown</button>
            <button class="btn" disabled={!chosen.length} onclick={() => act({ action: 'read' })}>Mark read</button>
            <button class="btn" disabled={!chosen.length} onclick={() => act({ action: 'unread' })}>Mark unread</button>
            <button class="btn" disabled={!chosen.length} onclick={() => act({ action: 'download' })}><Icon name="download" size={18} />Download unread</button>
            {#if data.categories.length}
              <label>
                <span class="sr">Add to a category</span>
                <select class="btn" disabled={!chosen.length} onchange={(e) => e.currentTarget.value && act({ action: 'categories', categories: [Number(e.currentTarget.value)] })}>
                  <option value="">Add to category…</option>
                  {#each data.categories as c (c.id)}<option value={c.id}>{c.name}</option>{/each}
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
            {#each shown as item (item.id)}
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
            {#each shown as item (item.id)}
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

  .bulk {
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
</style>
