<script lang="ts">
  import { get, message, type Count, type Stats } from '../lib/api';
  import Notice from '../lib/Notice.svelte';

  let data = $state<Stats | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);

  async function load() {
    error = null;
    try {
      data = await get<Stats>('/stats');
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  const most = (list: Count[]) => Math.max(1, ...list.map((entry) => entry.count));
  const monthName = (key: string) => {
    const [year, month] = key.split('-').map(Number);
    return new Date(year, month - 1, 1).toLocaleDateString(undefined, { month: 'short' });
  };
</script>

{#snippet bars(title: string, list: Count[])}
  <section class="panel block">
    <h2 class="display">{title}</h2>
    {#if list.length === 0}
      <p class="muted">Nothing yet.</p>
    {:else}
      <ul>
        {#each list as entry (entry.name)}
          <li>
            <span class="name">{entry.name}</span>
            <span class="bar" aria-hidden="true"><span style:width="{(entry.count / most(list)) * 100}%"></span></span>
            <strong>{entry.count}</strong>
          </li>
        {/each}
      </ul>
    {/if}
  </section>
{/snippet}

<div class="page">
  <div class="page-head">
    <h1 class="display">STATS</h1>
    <span class="muted">What your reading adds up to. Days are counted in UTC.</span>
  </div>

  <Notice {loading} {error} retry={load} />

  {#if data}
    <div class="tiles">
      {#each [['Titles', data.titles], ['Begun', data.started], ['Read to the end', data.finished], ['Chapters read', data.read], ['Days with reading', data.days], ['Days in a row', data.streak], ['Chapters saved', data.downloaded], ['Bookmarks', data.bookmarks], ['Kept pages', data.pageMarks]] as [label, value] (label)}
        <div class="panel tile"><span class="display">{value}</span><span>{label}</span></div>
      {/each}
    </div>

    <section class="panel block">
      <h2 class="display">CHAPTERS READ, MONTH BY MONTH</h2>
      <div class="months">
        {#each data.months as month (month.name)}
          <div class="month" title="{month.count} in {month.name}">
            <strong>{month.count || ''}</strong>
            <span class="column" style:height="{(month.count / most(data.months)) * 100}%"></span>
            <span class="label">{monthName(month.name)}</span>
          </div>
        {/each}
      </div>
    </section>

    <div class="split">
      {@render bars('SOURCES', data.sources)}
      {@render bars('GENRES', data.genres)}
      {@render bars('STATUS', data.statuses)}
    </div>
  {/if}
</div>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(150px, 1fr));
    gap: 14px;
  }

  .tile {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 14px;
  }

  .tile .display {
    font-size: 30px;
    line-height: 1.1;
  }

  .tile span:last-child {
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .block {
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 18px;
  }

  .block h2 {
    font-size: 18px;
  }

  .months {
    display: grid;
    grid-template-columns: repeat(12, minmax(0, 1fr));
    gap: 6px;
    height: 200px;
  }

  .month {
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: center;
    gap: 4px;
    min-width: 0;
  }

  .month strong {
    font-size: 11px;
  }

  .column {
    width: 100%;
    min-height: 2px;
    border: 2px solid var(--ink);
    background: var(--spot);
  }

  .label {
    font-size: 11px;
    font-weight: 700;
  }

  .split {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 18px;
    align-items: start;
  }

  ul {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    display: grid;
    grid-template-columns: minmax(0, 1.2fr) minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    font-size: 13px;
  }

  .name {
    overflow: hidden;
    font-weight: 700;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .bar {
    display: flex;
    height: 12px;
    border: 2px solid var(--ink);
  }

  .bar span {
    background: var(--ink);
  }
</style>
