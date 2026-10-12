<script lang="ts">
  import { get, message, send, type UpdateItem, type Updates } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { ago, dayKey, dayLabel } from '../lib/util';

  // What was here last time shows at once; the list is then asked for again.
  const kept = recall<Updates>('updates');
  let data = $state<Updates | null>(kept);
  let loading = $state(!kept);
  let error = $state<string | null>(null);
  let note = $state('');
  let showErrors = $state(false);

  const running = $derived(data?.status.running ?? false);
  const days = $derived.by(() => {
    const groups: { day: number; items: UpdateItem[] }[] = [];
    for (const item of data?.items ?? []) {
      const day = dayKey(item.fetchedAt);
      if (groups.at(-1)?.day === day) groups.at(-1)!.items.push(item);
      else groups.push({ day, items: [item] });
    }
    return groups;
  });

  async function load() {
    try {
      const fresh = await get<Updates>('/updates');
      keep('updates', fresh);
      data = fresh;
      error = null;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  // While a run is going, the screen follows it.
  $effect(() => {
    if (!running) return;
    const timer = setInterval(load, 2000);
    return () => clearInterval(timer);
  });

  async function run() {
    note = '';
    try {
      const status = await send<Updates['status']>('POST', '/updates/run');
      if (data) data.status = status;
    } catch (e) {
      error = message(e);
    }
  }

  async function download(item: UpdateItem) {
    try {
      await send('POST', '/downloads', { chapters: [item.chapterId] });
      note = `${item.title}, ${item.chapterName}: queued for download.`;
    } catch (e) {
      error = message(e);
    }
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display">UPDATES</h1>
    <div class="tools">
      {#if data?.status.finishedAt && !running}
        <strong>Checked {ago(data.status.finishedAt)}: {data.status.found} new{data.status.failed ? `, ${data.status.failed} failed` : ''}</strong>
      {/if}
      <button class="btn big spot" disabled={running} onclick={run}>
        <Icon name="refresh" size={18} stroke={3} />
        {running && data ? `Checking ${data.status.done} / ${data.status.total}` : 'Update now'}
      </button>
    </div>
  </div>

  <Notice {loading} {error} retry={load} />
  {#if note}<p role="status"><strong>{note}</strong> <a class="link" href="#/downloads">See downloads</a></p>{/if}

  {#if data?.errors.length}
    <section class="panel problems">
      <button class="btn" aria-expanded={showErrors} onclick={() => (showErrors = !showErrors)}>
        {data.errors.length} {data.errors.length === 1 ? 'title' : 'titles'} could not be checked<Icon name={showErrors ? 'up' : 'down'} size={18} />
      </button>
      {#if showErrors}
        <ul class="rows">
          {#each data.errors as failure (failure.mangaId)}
            <li>
              <a class="main" href="#/title/{failure.mangaId}"><strong>{failure.title}</strong><span>{failure.error} · {ago(failure.at)}</span></a>
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/if}

  {#each days as group (group.day)}
    <section>
      <div class="section-head"><h2 class="display">{dayLabel(group.day)}</h2></div>
      <ul class="rows">
        {#each group.items as item (item.chapterId)}
          <li class:done={item.read}>
            <a class="main" href="#/title/{item.mangaId}">
              <strong>{item.title}</strong>
              <span>{item.chapterName}{item.sourceName ? ` · ${item.sourceName}` : ''}{item.read ? ' · read' : ''}{item.downloaded ? ' · saved' : ''}</span>
            </a>
            <span class="tools">
              {#if !item.downloaded}
                <button class="btn icon" aria-label="Download {item.chapterName} of {item.title}" onclick={() => download(item)}><Icon name="download" /></button>
              {/if}
              <a class="btn" href="#/read/{item.chapterId}">Read<Icon name="arrow" size={18} stroke={3} /></a>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  {#if data && data.items.length === 0 && !loading}
    <Notice>No new chapters yet. Titles in the library are checked on a schedule; “Update now” checks them right away.</Notice>
  {/if}
</div>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .section-head h2 {
    font-size: 20px;
  }

  .problems {
    align-items: flex-start;
    padding: 12px;
    box-shadow: var(--sh-4);
  }

  .problems .rows {
    align-self: stretch;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    flex-wrap: wrap;
  }

  .done .main {
    color: var(--muted);
  }

  .link {
    text-decoration: underline;
  }
</style>
