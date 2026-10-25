<script lang="ts">
  // Many library titles moved to other sources at once. Each title is looked for in the
  // sources picked, in their order; the first that has it by the same name wins, else the
  // first that has anything. Nothing moves until the matches are confirmed.
  import { get, message, send, type Card, type Library, type LibraryItem, type Listing, type Source } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { browseLangs } from '../lib/prefs.svelte';
  import { langName } from '../lib/util';

  type Choice = 'chapters' | 'categories' | 'tracks' | 'keep';
  type Row = { title: LibraryItem; state: 'waiting' | 'looking' | 'found' | 'none' | 'moving' | 'moved' | 'failed'; match: Card | null; from: Source | null; on: boolean; error: string };

  let { ids }: { ids: number[] } = $props();

  const PATIENCE_MS = 30_000;
  const AT_ONCE = 2;

  let rows = $state<Row[]>([]);
  let sources = $state<Source[]>([]);
  let picked = $state<string[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let phase = $state<'pick' | 'looking' | 'review' | 'moving' | 'done'>('pick');
  let options = $state<Record<Choice, boolean>>({ chapters: true, categories: true, tracks: true, keep: false });

  const chosen = $derived(picked.map((id) => sources.find((s) => s.id === id)).filter((s): s is Source => !!s));
  const ready = $derived(rows.filter((row) => row.on && row.match));

  /** Letters and digits only, in lower case: how two sources' names for one title are compared. */
  const plain = (name: string) => name.toLowerCase().replace(/[^\p{L}\p{N}]/gu, '');

  async function load() {
    try {
      const [library, found] = await Promise.all([get<Library>('/library'), get<{ sources: Source[] }>('/sources')]);
      rows = library.items.filter((item) => ids.includes(item.id)).map((title) => ({ title, state: 'waiting', match: null, from: null, on: true, error: '' }));
      sources = found.sources.filter((s) => !s.hidden && s.id !== '0');
      const langs = browseLangs(sources);
      const pinned = sources.filter((s) => s.pinned);
      picked = (pinned.length ? pinned : sources.filter((s) => langs.length === 0 || langs.includes(s.lang))).slice(0, 5).map((s) => s.id);
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  function toggleSource(id: string) {
    picked = picked.includes(id) ? picked.filter((other) => other !== id) : [...picked, id];
  }

  function moveSource(at: number, by: number) {
    const next = [...picked];
    const [id] = next.splice(at, 1);
    next.splice(at + by, 0, id);
    picked = next;
  }

  async function search(source: Source, name: string): Promise<Card[]> {
    const stop = new AbortController();
    const timer = setTimeout(() => stop.abort(), PATIENCE_MS);
    try {
      return (await get<Listing>(`/sources/${source.id}/search?q=${encodeURIComponent(name)}`, stop.signal)).items;
    } catch {
      return [];
    } finally {
      clearTimeout(timer);
    }
  }

  async function look(row: Row) {
    row.state = 'looking';
    let fallback: [Card, Source] | null = null;
    for (const source of chosen) {
      const found = (await search(source, row.title.title)).filter((card) => card.id !== row.title.id);
      const same = found.find((card) => plain(card.title) === plain(row.title.title));
      if (same) {
        [row.match, row.from] = [same, source];
        break;
      }
      fallback ??= found.length ? [found[0], source] : null;
    }
    if (!row.match && fallback) [row.match, row.from] = fallback;
    row.state = row.match ? 'found' : 'none';
    row.on = !!row.match;
  }

  async function findAll() {
    phase = 'looking';
    let next = 0;
    const worker = async () => {
      while (next < rows.length) await look(rows[next++]);
    };
    await Promise.all(Array.from({ length: Math.min(AT_ONCE, rows.length) }, worker));
    phase = 'review';
  }

  async function moveAll() {
    phase = 'moving';
    for (const row of ready) {
      row.state = 'moving';
      try {
        await send('POST', `/manga/${row.title.id}/migrate/${row.match!.id}`, options);
        row.state = 'moved';
      } catch (e) {
        row.state = 'failed';
        row.error = message(e);
      }
    }
    phase = 'done';
  }

  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
</script>

{#snippet choice(key: Choice, label: string)}
  <button class="btn" aria-pressed={options[key]} onclick={() => (options[key] = !options[key])}>
    {#if options[key]}<Icon name="check" size={18} stroke={3} />{/if}{label}
  </button>
{/snippet}

<div class="page">
  <div class="page-head">
    <h1 class="display">MOVE {rows.length || ''} TITLES</h1>
    <a class="btn" href="#/"><Icon name="back" size={16} stroke={3} />Back to the library</a>
  </div>

  <Notice {loading} {error} retry={load} />

  {#if !loading && !error}
    {#if phase === 'pick'}
      <section class="panel block">
        <strong>Where to look, in this order</strong>
        <p class="muted">Each title is looked for in these sources, one after another. The first that has it by the same name is taken.</p>
        <ul class="rows">
          {#each chosen as source, at (source.id)}
            <li>
              <span class="main"><strong>{at + 1}. {source.name}</strong><span>{langName(source.lang)}</span></span>
              <span class="tools">
                <button class="btn icon" aria-label="Move {source.name} up" disabled={at === 0} onclick={() => moveSource(at, -1)}><Icon name="up" /></button>
                <button class="btn icon" aria-label="Move {source.name} down" disabled={at === chosen.length - 1} onclick={() => moveSource(at, 1)}><Icon name="down" /></button>
                <button class="btn icon" aria-label="Leave {source.name} out" onclick={() => toggleSource(source.id)}><Icon name="close" /></button>
              </span>
            </li>
          {/each}
        </ul>
        <label class="field">
          <span class="sr">Add a source</span>
          <select onchange={(e) => (e.currentTarget.value && toggleSource(e.currentTarget.value), (e.currentTarget.value = ''))}>
            <option value="">Add a source…</option>
            {#each sources.filter((s) => !picked.includes(s.id)) as source (source.id)}
              <option value={source.id}>{source.name} · {langName(source.lang)}</option>
            {/each}
          </select>
        </label>
        <button class="btn big spot" disabled={!chosen.length || !rows.length} onclick={findAll}>Find {count(rows.length, 'title', 'titles')}</button>
      </section>
    {/if}

    {#if phase !== 'pick'}
      <ul class="rows">
        {#each rows as row (row.title.id)}
          <li class:off={!row.on}>
            <span class="art"><Cover src={row.title.cover} title={row.title.title} /></span>
            <span class="main">
              <strong>{row.title.title}</strong>
              <span>
                {#if row.state === 'looking' || row.state === 'waiting'}Looking…
                {:else if row.state === 'none'}Not found in the sources picked.
                {:else if row.match}{row.match.title} · {row.from?.name}{/if}
                {#if row.state === 'moving'} · moving…{:else if row.state === 'moved'} · moved{:else if row.state === 'failed'} · {row.error}{/if}
              </span>
            </span>
            {#if row.match}<span class="art"><Cover src={row.match.cover} title={row.match.title} /></span>{/if}
            {#if phase === 'review'}
              <span class="tools">
                {#if row.match}
                  <button class="btn" aria-pressed={row.on} onclick={() => (row.on = !row.on)}>{row.on ? 'Move' : 'Leave'}</button>
                {/if}
                <a class="btn" href="#/search?q={encodeURIComponent(row.title.title)}&for={row.title.id}">Pick by hand</a>
              </span>
            {/if}
          </li>
        {/each}
      </ul>
    {/if}

    {#if phase === 'review'}
      <section class="panel block">
        <strong>What goes along</strong>
        <div class="tools">
          {@render choice('chapters', 'Reading progress')}
          {@render choice('categories', 'Categories')}
          {@render choice('tracks', 'Tracker links')}
          {@render choice('keep', 'Keep the old titles too')}
        </div>
        <button class="btn big spot" disabled={!ready.length} onclick={moveAll}>Move {count(ready.length, 'title', 'titles')}</button>
      </section>
    {:else if phase === 'done'}
      <Notice>
        {count(rows.filter((r) => r.state === 'moved').length, 'title was', 'titles were')} moved.
        <a class="btn spot" href="#/">Back to the library</a>
      </Notice>
    {/if}
  {/if}
</div>

<style>
  .block {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 18px;
  }

  .block > .btn.big {
    align-self: flex-start;
  }

  .art {
    flex: 0 0 44px;
  }

  .off {
    opacity: 0.55;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  select {
    font: inherit;
  }
</style>
