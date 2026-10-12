<script lang="ts">
  import { get, message, type FilterChange, type SourceFilter } from './api';
  import Icon from './Icon.svelte';
  import Notice from './Notice.svelte';

  // What a source's search can be narrowed by, drawn from the source's own list.
  let { id, onapply }: { id: string; onapply: (changes: FilterChange[]) => void } = $props();

  let filters = $state<SourceFilter[]>([]);
  // The list as the source made it, to tell what was set to something else.
  let made: SourceFilter[] = [];
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    loading = true;
    error = null;
    try {
      made = (await get<{ filters: SourceFilter[] }>(`/sources/${id}/filters`)).filters;
      filters = structuredClone(made);
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  /** Only what differs from the source's list is sent: a filter is named by its place in it. */
  function changes(now: SourceFilter[], was: SourceFilter[]): FilterChange[] {
    const out: FilterChange[] = [];
    now.forEach((f, position) => {
      const old = was[position];
      if (f.type === 'group') {
        const inner = changes(f.filters, old.type === 'group' ? old.filters : []);
        if (inner.length) out.push({ position, state: inner });
      } else if ('state' in f && JSON.stringify(f.state) !== JSON.stringify('state' in old ? old.state : undefined)) {
        out.push({ position, state: f.state });
      }
    });
    return out;
  }

  function apply(event: SubmitEvent) {
    event.preventDefault();
    onapply(changes(filters, made));
  }

  /** Back to the list as the source makes it now. Some fill theirs in only after a first look at the site. */
  async function reset() {
    await load();
    if (!error) onapply([]);
  }

  function sortBy(f: Extract<SourceFilter, { type: 'sort' }>, index: number) {
    f.state = { index, ascending: f.state?.ascending ?? false };
  }

  /** How many of a group's filters are switched on, to show while it is folded. */
  function on(group: SourceFilter[]): number {
    return group.filter((f) => (f.type === 'checkbox' && f.state) || (f.type === 'tristate' && f.state !== 0)).length;
  }

  const TRI = ['either way', 'with it', 'without it'];
</script>

{#snippet list(items: SourceFilter[])}
  {#each items as f, i (i)}
    {#if f.type === 'header'}
      <p class="head">{f.name}</p>
    {:else if f.type === 'separator'}
      <hr />
    {:else if f.type === 'select'}
      <label class="row">
        <span>{f.name}</span>
        <select class="btn" bind:value={f.state}>
          {#each f.values as word, at (at)}<option value={at}>{word}</option>{/each}
        </select>
      </label>
    {:else if f.type === 'text'}
      <label class="row">
        <span>{f.name}</span>
        <span class="field"><input bind:value={f.state} /></span>
      </label>
    {:else if f.type === 'checkbox'}
      <button type="button" class="btn pill" aria-pressed={f.state} onclick={() => (f.state = !f.state)}>{f.name}</button>
    {:else if f.type === 'tristate'}
      <button
        type="button"
        class="btn pill"
        aria-pressed={f.state === 1 ? 'true' : f.state === 2 ? 'mixed' : 'false'}
        aria-label="{f.name}: {TRI[f.state] ?? TRI[0]}"
        onclick={() => (f.state = (f.state + 1) % 3)}
      >
        {#if f.state === 1}<Icon name="check" size={16} />{:else if f.state === 2}<Icon name="close" size={16} />{/if}{f.name}
      </button>
    {:else if f.type === 'sort'}
      <div class="row">
        <span>{f.name}</span>
        <span class="tools">
          <select class="btn" aria-label={f.name} value={f.state?.index ?? -1} onchange={(e) => sortBy(f, Number(e.currentTarget.value))}>
            {#if !f.state}<option value={-1}>As the source sorts</option>{/if}
            {#each f.values as word, at (at)}<option value={at}>{word}</option>{/each}
          </select>
          <button
            type="button"
            class="btn icon"
            disabled={!f.state}
            aria-label={f.state?.ascending ? 'Sorted upwards. Sort downwards' : 'Sorted downwards. Sort upwards'}
            onclick={() => f.state && (f.state.ascending = !f.state.ascending)}
          >
            <Icon name={f.state?.ascending ? 'up' : 'down'} />
          </button>
        </span>
      </div>
    {:else if f.type === 'group'}
      <details class="group">
        <summary>{f.name}{#if on(f.filters)}<span class="badge">{on(f.filters)}</span>{/if}</summary>
        <div class="in">{@render list(f.filters)}</div>
      </details>
    {/if}
  {/each}
{/snippet}

<form class="panel filters" onsubmit={apply}>
  <Notice {loading} {error} retry={load} />
  {#if !loading && !error}
    {#if filters.length === 0}
      <p class="muted">This source has no filters.</p>
    {:else}
      <div class="in">{@render list(filters)}</div>
      <div class="tools ends">
        <button type="button" class="btn" onclick={reset}>Reset</button>
        <button class="btn ink"><Icon name="search" size={18} />Search</button>
      </div>
    {/if}
  {/if}
</form>

<style>
  .filters {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 16px;
  }

  .in {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  .head,
  hr,
  .row,
  .group {
    flex: 1 1 100%;
  }

  .head {
    font-size: 12.5px;
    font-weight: 700;
    overflow-wrap: anywhere;
    white-space: pre-line;
  }

  hr {
    margin: 4px 0;
    border: 0;
    border-top: 2px solid var(--ink);
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
    font-size: 15px;
    font-weight: 900;
  }

  .row .field {
    flex: 1 1 220px;
    box-shadow: none;
  }

  .group {
    border: 2px solid var(--ink);
  }

  summary {
    padding: 11px 14px;
    font-weight: 900;
    cursor: pointer;
  }

  summary .badge {
    margin-left: 8px;
  }

  .group .in {
    padding: 4px 14px 14px;
  }

  .btn[aria-pressed='mixed'] {
    text-decoration: line-through;
  }

  .ends {
    justify-content: flex-end;
  }

  .muted {
    font-weight: 700;
  }
</style>
