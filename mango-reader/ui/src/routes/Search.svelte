<script lang="ts">
  import { get, message, send, type Card, type Listing, type Source } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { browseLangs } from '../lib/prefs.svelte';
  import { go } from '../lib/router.svelte';
  import TitleCard from '../lib/TitleCard.svelte';
  import { langName } from '../lib/util';

  // Each source is asked on its own, a few at a time. One that is slow or broken
  // only holds up its own row.
  const AT_ONCE = 5;
  const MOST = 20;
  const PATIENCE_MS = 30_000;

  type Row = { source: Source; state: 'waiting' | 'searching' | 'done' | 'failed'; items: Card[]; error: string };

  // With `from`, the search picks the title that one moves to: a card leads to the
  // comparison of the two, and the title itself is left out of what is found.
  let { q, from = null }: { q: string; from?: number | null } = $props();

  let rows = $state<Row[]>([]);
  let scope = $state('');
  let error = $state<string | null>(null);
  let left = $state(0);

  async function ask(row: Row) {
    row.state = 'searching';
    const stop = new AbortController();
    const timer = setTimeout(() => stop.abort(), PATIENCE_MS);
    try {
      row.items = (await get<Listing>(`/sources/${row.source.id}/search?q=${encodeURIComponent(q)}`, stop.signal)).items.filter((card) => card.id !== from);
      row.state = 'done';
    } catch (e) {
      row.error = stop.signal.aborted ? 'No answer after 30 seconds.' : message(e);
      row.state = 'failed';
    }
    clearTimeout(timer);
  }

  async function run() {
    if (!q) return;
    try {
      const all = (await get<{ sources: Source[] }>('/sources')).sources.filter((s) => !s.hidden);
      const langs = browseLangs(all);
      const pinned = all.filter((s) => s.pinned);
      // Pinned sources if there are any; otherwise those in the languages picked on Browse.
      const pool = pinned.length ? pinned : all.filter((s) => langs.length === 0 || langs.includes(s.lang));
      scope = pinned.length ? 'your pinned sources' : langs.length ? 'sources in your languages' : 'your sources';
      left = Math.max(0, pool.length - MOST);
      rows = pool.slice(0, MOST).map((source) => ({ source, state: 'waiting', items: [], error: '' }));
    } catch (e) {
      error = message(e);
      return;
    }
    let next = 0;
    const worker = async () => {
      while (next < rows.length) await ask(rows[next++]);
    };
    await Promise.all(Array.from({ length: Math.min(AT_ONCE, rows.length) }, worker));
  }
  run();

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
    if (text) go(`/search?q=${encodeURIComponent(text)}&for=${from}`);
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display">{from ? 'MOVE' : 'SEARCH'}</h1>
    <strong>“{q}” in {scope || '…'}</strong>
  </div>

  {#if from}
    <div class="pick">
      <strong>Pick the title to move to. Nothing changes until you confirm.</strong>
      <form class="field" role="search" onsubmit={again}>
        <Icon name="search" size={18} stroke={3} />
        <input type="search" name="q" value={q} placeholder="Try another name" aria-label="Search for another name" />
      </form>
      <a class="btn" href="#/title/{from}"><Icon name="back" size={16} stroke={3} />Back to the title</a>
    </div>
  {/if}

  <Notice {error} />
  {#if left > 0}
    <Notice>
      {left} more sources were left out. Pin the ones you want searched, or pick languages on Browse.
      <a class="btn" href="#/browse">Open Browse</a>
    </Notice>
  {/if}
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
            <TitleCard {...card} href={from ? `#/migrate/${from}/${card.id}` : undefined} onLibrary={from ? undefined : () => toggle(card)} />
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
    grid-auto-columns: minmax(130px, 150px);
    grid-auto-flow: column;
    gap: 24px;
    overflow-x: auto;
    padding: 14px 12px 14px 2px;
  }
</style>
