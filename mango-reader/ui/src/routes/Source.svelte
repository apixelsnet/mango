<script lang="ts">
  import { get, message, send, type Card, type Listing, type Source } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import TitleCard from '../lib/TitleCard.svelte';
  import { langName, whenVisible } from '../lib/util';

  type Tab = 'popular' | 'latest' | 'search';

  let { id }: { id: string } = $props();

  let source = $state<Source | null>(null);
  let tab = $state<Tab>('popular');
  let typed = $state('');
  let query = $state('');
  let items = $state<Card[]>([]);
  let page = 0;
  let more = $state(false);
  let loading = $state(false);
  let error = $state<string | null>(null);
  // Answers to a list the user has already left are dropped.
  let current = 0;

  async function load(fresh: boolean) {
    if (fresh) {
      items = [];
      page = 0;
      more = false;
    }
    const mine = ++current;
    loading = true;
    error = null;
    try {
      const search = tab === 'search' ? `&q=${encodeURIComponent(query)}` : '';
      const list = await get<Listing>(`/sources/${id}/${tab}?page=${page + 1}${search}`);
      if (mine !== current) return;
      const seen = new Set(items.map((card) => card.id));
      items.push(...list.items.filter((card) => !seen.has(card.id)));
      page += 1;
      more = list.hasNextPage;
    } catch (e) {
      if (mine !== current) return;
      error = message(e);
    }
    loading = false;
  }

  function open(next: Tab) {
    tab = next;
    load(true);
  }

  function search(event: SubmitEvent) {
    event.preventDefault();
    query = typed.trim();
    if (query) open('search');
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

  get<{ sources: Source[] }>('/sources')
    .then((all) => (source = all.sources.find((s) => s.id === id) ?? null))
    .catch(() => {});
  load(true);
</script>

<div class="page">
  <div class="page-head">
    <div>
      <a class="crumb" href="#/browse"><Icon name="back" size={16} stroke={3} />Browse</a>
      <h1 class="display">{source?.name ?? 'Source'}</h1>
      {#if source}<span class="tag">{langName(source.lang)}</span>{/if}
    </div>
    <form class="field" role="search" onsubmit={search}>
      <Icon name="search" size={18} stroke={3} />
      <input type="search" bind:value={typed} placeholder="Search this source" aria-label="Search this source" />
    </form>
  </div>

  <div class="section-head">
    <div class="tools" role="group" aria-label="List">
      <button class="btn pill" aria-pressed={tab === 'popular'} onclick={() => open('popular')}>Popular</button>
      {#if source?.supportsLatest}
        <button class="btn pill" aria-pressed={tab === 'latest'} onclick={() => open('latest')}>Latest</button>
      {/if}
      {#if tab === 'search'}
        <button class="btn pill" aria-pressed="true" onclick={() => open('search')}>“{query}”</button>
      {/if}
    </div>
    <button class="btn icon" aria-label="Load this list again" onclick={() => load(true)}><Icon name="refresh" /></button>
  </div>

  <div class="grid">
    {#each items as card (card.id)}
      <TitleCard {...card} onLibrary={() => toggle(card)} />
    {/each}
  </div>

  <Notice {loading} {error} retry={() => load(items.length === 0)} />
  {#if !loading && !error && items.length === 0}
    <Notice>Nothing here.</Notice>
  {/if}
  {#if more && !loading && !error}
    <div class="more" use:whenVisible={() => more && !loading && load(false)}>
      <button class="btn" onclick={() => load(false)}>Load more</button>
    </div>
  {/if}
</div>

<style>
  .page-head > div {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .page-head h1 {
    font-size: clamp(30px, 6vw, 52px);
    overflow-wrap: anywhere;
  }

  .crumb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 32px;
    font-size: 13px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  form {
    flex: 0 1 340px;
  }

  .more {
    display: flex;
    justify-content: center;
  }
</style>
