<script lang="ts">
  import { get, message, send, type Comparison, type MigrateSide } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { go } from '../lib/router.svelte';
  import { langName } from '../lib/util';

  type Choice = 'chapters' | 'categories' | 'tracks' | 'keep';

  let { from, to }: { from: number; to: number } = $props();

  let seen = $state<Comparison | null>(null);
  let loading = $state(true);
  let busy = $state(false);
  let error = $state<string | null>(null);
  // What goes along. `keep` leaves the old title in the library: a copy, not a move.
  let options = $state<Record<Choice, boolean>>({ chapters: true, categories: true, tracks: true, keep: false });

  // The new source stops before the place reached on the old one.
  const short = $derived(!!seen && seen.from.lastRead >= 0 && seen.to.latest < seen.from.lastRead);

  // Mango asks the other source for its chapters before it answers, so this can take a while.
  async function load() {
    loading = true;
    error = null;
    try {
      seen = await get<Comparison>(`/manga/${from}/migrate/${to}`);
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  async function move() {
    busy = true;
    error = null;
    try {
      await send('POST', `/manga/${from}/migrate/${to}`, options);
      go(`/title/${to}`, true);
    } catch (e) {
      error = message(e);
      busy = false;
    }
  }

  const upTo = (number: number) => (number >= 0 ? `, up to Ch. ${+number.toFixed(2)}` : '');
  const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
</script>

{#snippet side(label: string, title: MigrateSide)}
  <a class="panel side" href="#/title/{title.id}">
    <span class="art"><Cover src={title.cover} title={title.title} /></span>
    <span class="facts">
      <span class="tools">
        <span class="tag ink">{label}</span>
        {#if title.sourceLang}<span class="tag">{langName(title.sourceLang)}</span>{/if}
        {#if title.inLibrary}<span class="tag">In library</span>{/if}
      </span>
      <strong class="name">{title.title}</strong>
      <strong>{title.sourceName ?? 'Its extension is not installed'}</strong>
      <span>{count(title.chapters, 'chapter', 'chapters')}{upTo(title.latest)}</span>
      <span>{title.read} read{upTo(title.lastRead)}</span>
    </span>
  </a>
{/snippet}

{#snippet choice(key: Choice, label: string, about: string)}
  <li>
    <span class="main"><strong>{label}</strong><span>{about}</span></span>
    <button class="btn" aria-pressed={options[key]} aria-label={label} onclick={() => (options[key] = !options[key])}>
      {#if options[key]}<Icon name="check" size={18} stroke={3} />{/if}{options[key] ? 'Yes' : 'No'}
    </button>
  </li>
{/snippet}

<div class="page">
  <div class="page-head">
    <h1 class="display">MOVE</h1>
    <a class="btn" href="#/title/{from}"><Icon name="back" size={16} stroke={3} />Back to the title</a>
  </div>

  <Notice {loading} {error} retry={seen ? move : load} />

  {#if seen}
    <div class="pair">
      {@render side('From', seen.from)}
      {@render side('To', seen.to)}
    </div>

    {#if seen.to.chapters === 0}
      <Notice>This source lists no chapters for it, so no progress can go along.</Notice>
    {:else if short}
      <Notice>You have read{upTo(seen.from.lastRead).slice(1)}, and this source stops earlier. What you read past its last chapter is not carried over.</Notice>
    {/if}

    <section class="what">
      <div class="section-head"><h2 class="display">WHAT GOES ALONG</h2></div>
      <ul class="rows">
        {@render choice(
          'chapters',
          'Reading progress',
          `${count(seen.read, 'chapter', 'chapters')} there will be marked read. Bookmarks, the page reached and the history follow.`,
        )}
        {@render choice('categories', 'Categories', seen.categories ? `${count(seen.categories, 'category', 'categories')} to add.` : 'None to add.')}
        {@render choice('tracks', 'Tracker links', seen.tracks ? `${count(seen.tracks, 'link', 'links')}.` : 'None.')}
        {@render choice('keep', 'Keep the old one in the library', 'A copy instead of a move: both stay in the library.')}
      </ul>
      {#if seen.from.downloaded}
        <p><strong>{count(seen.from.downloaded, 'saved chapter stays', 'saved chapters stay')} with the old title.</strong> Delete them on its page first if you want the space back.</p>
      {/if}
      <div class="tools">
        <button class="btn big spot" disabled={busy} onclick={move}>{options.keep ? 'Copy' : 'Move'}<Icon name="arrow" size={18} stroke={3} /></button>
        <a class="btn big" href="#/search?q={encodeURIComponent(seen.from.title)}&for={from}">Pick another</a>
      </div>
    </section>
  {/if}
</div>

<style>
  .pair {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(min(280px, 100%), 1fr));
    gap: 24px;
  }

  .side {
    display: flex;
    gap: 18px;
    padding: 16px;
  }

  .art {
    flex: 0 0 clamp(80px, 26%, 120px);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 8px;
    min-width: 0;
    font-size: 13px;
    font-weight: 700;
  }

  .name {
    font-size: 18px;
    font-weight: 900;
    overflow-wrap: anywhere;
  }

  .what {
    display: flex;
    flex-direction: column;
    gap: 16px;
    max-width: 760px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .rows .btn {
    flex: none;
    min-width: 84px;
  }
</style>
