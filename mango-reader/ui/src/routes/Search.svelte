<script lang="ts">
  import { get, message, send, type Card, type Listing, type Source } from '../lib/api';
  import { onDestroy } from 'svelte';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { browseLangs } from '../lib/prefs.svelte';
  import { go, route } from '../lib/router.svelte';
  import TitleCard from '../lib/TitleCard.svelte';
  import { langName } from '../lib/util';

  // Each source is asked on its own, a few at a time. One that is slow or broken
  // only holds up its own row.
  const AT_ONCE = 5;
  const PATIENCE_MS = 30_000;

  type Row = { source: Source; state: 'waiting' | 'searching' | 'done' | 'failed'; items: Card[]; error: string };

  // With `from`, the search picks the title that one moves to: a card leads to the
  // comparison of the two, and the title itself is left out of what is found.
  // With `merge`, it picks a title to read as another source of that one.
  // With `everywhere`, every source is searched even when some are pinned.
  let {
    q,
    from = null,
    merge = null,
    everywhere = false,
  }: { q: string; from?: number | null; merge?: number | null; everywhere?: boolean } = $props();
  const away = $derived(from ?? merge);
  const key = $derived(`${q}|${from}|${merge}|${everywhere}`);

  type Kept = { key: string; rows: Row[]; scope: string; pinned: number };

  /** Come back to, a search shows what it found, instead of asking every source again. */
  function before(): Kept | null {
    const was = route.returned ? recall<Kept>('search') : null;
    return was?.key === key ? was : null;
  }
  const kept = before();

  let rows = $state<Row[]>(kept?.rows ?? []);
  let scope = $state(kept?.scope ?? '');
  let error = $state<string | null>(null);
  // How many sources are pinned: with none, there is nothing to choose between.
  let pinnedCount = $state(kept?.pinned ?? 0);

  /** The address of this search, for other words or the other set of sources. */
  function link(text: string, all: boolean): string {
    return `/search?q=${encodeURIComponent(text)}${from ? `&for=${from}` : ''}${merge ? `&merge=${merge}` : ''}${all ? '&in=all' : ''}`;
  }

  // Leaving the page stops the searches still out: what they find would go nowhere,
  // and the sources not asked yet are asked on coming back.
  const leaving = new AbortController();

  async function ask(row: Row) {
    const skip = away;
    row.state = 'searching';
    const stop = new AbortController();
    const timer = setTimeout(() => stop.abort(), PATIENCE_MS);
    const left = () => stop.abort();
    leaving.signal.addEventListener('abort', left);
    try {
      row.items = (await get<Listing>(`/sources/${row.source.id}/search?q=${encodeURIComponent(q)}`, stop.signal)).items.filter((card) => card.id !== skip);
      row.state = 'done';
    } catch (e) {
      if (leaving.signal.aborted) return;
      row.error = stop.signal.aborted ? 'No answer after 30 seconds.' : message(e);
      row.state = 'failed';
    } finally {
      clearTimeout(timer);
      leaving.signal.removeEventListener('abort', left);
    }
  }

  async function run() {
    if (!q) return;
    if (!kept) {
      try {
        const all = (await get<{ sources: Source[] }>('/sources')).sources.filter((s) => !s.hidden);
        const langs = browseLangs(all);
        const pinned = all.filter((s) => s.pinned);
        pinnedCount = pinned.length;
        // Pinned sources if there are any, unless all were asked for; otherwise those in the languages picked on Browse.
        const onlyPinned = pinned.length > 0 && !everywhere;
        const pool = onlyPinned ? pinned : all.filter((s) => langs.length === 0 || langs.includes(s.lang));
        scope = onlyPinned ? 'your pinned sources' : langs.length ? 'all sources in your languages' : 'all your sources';
        rows = pool.map((source) => ({ source, state: 'waiting', items: [], error: '' }));
      } catch (e) {
        error = message(e);
        return;
      }
    }
    // A search left before every source had answered asks the rest now.
    const due = rows.filter((row) => row.state === 'waiting' || row.state === 'searching');
    let next = 0;
    const worker = async () => {
      while (next < due.length && !leaving.signal.aborted) await ask(due[next++]);
    };
    await Promise.all(Array.from({ length: Math.min(AT_ONCE, due.length) }, worker));
  }
  run();
  onDestroy(() => {
    keep('search', $state.snapshot({ key, rows, scope, pinned: pinnedCount }));
    leaving.abort();
  });

  let merging = $state(false);

  /** Reads the picked title as another source of `merge`. Its chapters are fetched first, which can take a while. */
  async function addSource(card: Card) {
    if (!merge || merging) return;
    merging = true;
    error = null;
    try {
      await send('POST', `/manga/${merge}/merge/${card.id}`);
      go(`/title/${merge}`, true);
    } catch (e) {
      error = message(e);
    }
    merging = false;
  }

  async function toggle(card: Card) {
    card.inLibrary = !card.inLibrary;
    try {
      await send('PUT', `/manga/${card.id}/library`, { inLibrary: card.inLibrary });
    } catch (e) {
      card.inLibrary = !card.inLibrary;
      error = message(e);
    }
  }

  /** Sources often know a title under another name. */
  function again(event: SubmitEvent) {
    event.preventDefault();
    const text = String(new FormData(event.currentTarget as HTMLFormElement).get('q') ?? '').trim();
    if (text) go(link(text, everywhere));
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display caps">{from ? 'Move' : merge ? 'Add a source' : 'Search'}</h1>
    <strong>“{q}” in {scope || '…'}</strong>
  </div>

  <div class="pick">
    {#if from}<strong>Pick the title to move to. Nothing changes until you confirm.</strong>{/if}
    {#if merge}<strong>{merging ? 'Adding it…' : 'Pick the same title on another source. Its chapters then show with this one’s.'}</strong>{/if}
    <form class="field" role="search" onsubmit={again}>
      <Icon name="search" size={18} stroke={3} />
      <input
        type="search"
        name="q"
        value={q}
        placeholder={away ? 'Try another name' : 'Search all sources'}
        aria-label={away ? 'Search for another name' : 'Search all sources'}
        enterkeyhint="search"
      />
    </form>
    {#if pinnedCount > 0}
      <div class="tools" role="group" aria-label="Sources to search">
        <button class="btn pill" aria-pressed={!everywhere} onclick={() => everywhere && go(link(q, false), true)}>
          <Icon name="pin" size={16} />Pinned
        </button>
        <button class="btn pill" aria-pressed={everywhere} onclick={() => !everywhere && go(link(q, true), true)}>
          <Icon name="globe" size={16} />All
        </button>
      </div>
    {/if}
    {#if away}<a class="btn" href="#/title/{away}"><Icon name="back" size={16} stroke={3} />Back to the title</a>{/if}
  </div>

  <Notice {error} />
  {#if !error && rows.length === 0 && scope}
    <Notice>
      There is no source to search yet.
      <a class="btn spot" href="#/extensions">Open extensions</a>
    </Notice>
  {/if}

  {#each rows as row (row.source.id)}
    <section>
      <div class="section-head">
        <h2 class="display"><a href="#/source/{row.source.id}">{row.source.name}</a></h2>
        <span class="tag">{langName(row.source.lang)}</span>
      </div>
      {#if row.state === 'failed'}
        <Notice error={row.error} retry={() => ask(row)} />
      {:else if row.state === 'done' && row.items.length === 0}
        <p class="muted quiet">Nothing found.</p>
      {:else if row.state === 'done'}
        <div class="strip">
          {#each row.items as card (card.id)}
            <TitleCard
              {...card}
              href={from ? `#/migrate/${from}/${card.id}` : undefined}
              pick={merge ? () => addSource(card) : undefined}
              onLibrary={away ? undefined : () => toggle(card)}
            />
          {/each}
        </div>
      {:else}
        <Notice loading />
      {/if}
    </section>
  {/each}
</div>

<style>
  .pick {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 20px;
  }

  .pick form {
    flex: 1 1 220px;
    max-width: 420px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 14px;
  }

  .section-head h2 {
    font-size: 20px;
  }

  .quiet {
    font-weight: 700;
  }

  /* One row per source, scrolled sideways. */
  .strip {
    display: grid;
    grid-auto-columns: minmax(min(130px, var(--cover)), min(150px, var(--cover)));
    grid-auto-flow: column;
    gap: 24px;
    overflow-x: auto;
    padding: 14px 12px 14px 2px;
  }

  /* On a phone the row runs out to the screen's edges, with the next cover showing that there is more. */
  @media (max-width: 640px) {
    .strip {
      gap: 16px;
      margin: 0 calc(-1 * var(--gutter));
      padding: 14px var(--gutter);
    }
  }
</style>
