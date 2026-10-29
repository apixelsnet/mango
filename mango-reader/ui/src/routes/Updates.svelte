<script lang="ts">
  import { get, message, send, type Queued, type UpdateItem, type Updates } from '../lib/api';
  import DownloadTag from '../lib/DownloadTag.svelte';
  import Icon from '../lib/Icon.svelte';
  import { keep, recall } from '../lib/keep';
  import Notice from '../lib/Notice.svelte';
  import { left, moving, queueOf, waiting, type Waits } from '../lib/queue';
  import { ago, dayKey, dayLabel, every } from '../lib/util';

  // What was here last time shows at once; the list is then asked for again.
  const kept = recall<Updates>('updates');
  let data = $state<Updates | null>(kept);
  let loading = $state(!kept);
  let error = $state<string | null>(null);
  let note = $state('');
  let showErrors = $state(false);
  // The titles whose new chapters of a day are shown one by one, by `day:title`.
  let opened = $state<Record<string, boolean>>({});

  const running = $derived(data?.status.running ?? false);
  // By day, then by title within the day: a title that got five chapters is one row of five.
  const days = $derived.by(() => {
    const groups: { day: number; titles: { mangaId: number; items: UpdateItem[] }[] }[] = [];
    for (const item of data?.items ?? []) {
      const day = dayKey(item.fetchedAt);
      if (groups.at(-1)?.day !== day) groups.push({ day, titles: [] });
      const titles = groups.at(-1)!.titles;
      const same = titles.find((title) => title.mangaId === item.mangaId);
      if (same) same.items.push(item);
      else titles.push({ mangaId: item.mangaId, items: [item] });
    }
    return groups;
  });

  async function load() {
    try {
      const fresh = await get<Updates>('/updates');
      // A run that just ended may have queued chapters of titles set to download them.
      const ended = running && !fresh.status.running;
      keep('updates', fresh);
      data = fresh;
      error = null;
      if (ended) loadQueue();
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load().then(loadQueue);

  // While a run is going, the screen follows it, as long as it is in view.
  $effect(() => {
    if (!running) return;
    return every(2000, load);
  });

  // ---------- the download queue ----------

  // The chapters listed here that are in the download queue, by chapter, and why the queue waits, if it does.
  let queued = $state<Record<number, Queued>>({});
  let waits = $state<Waits>('');
  const inQueue = $derived(Object.keys(queued).length);
  $effect(() => {
    if (moving(queued)) return every(2000, loadQueue);
  });

  /** Reads which listed chapters are in the queue. One that has left it since was saved, so the list is read again. */
  async function loadQueue() {
    const found = await queueOf((data?.items ?? []).map((i) => i.chapterId));
    if (!found) return;
    const gone = left(queued, found.entries);
    queued = found.entries;
    waits = found.waits;
    if (gone) await load();
  }

  async function download(items: UpdateItem[]) {
    const list = items.filter((i) => !i.downloaded && !queued[i.chapterId]);
    // Shown as queued at once; the queue itself says what really happened.
    for (const item of list) queued[item.chapterId] = waiting(item.chapterId, item.chapterName, item.mangaId, item.title);
    try {
      const answer = await send<{ queued: number }>('POST', '/downloads', { chapters: list.map((i) => i.chapterId) });
      note = answer.queued ? '' : 'Already saved or queued.';
    } catch (e) {
      error = message(e);
    }
    await loadQueue();
  }

  async function cancelDownload(item: UpdateItem) {
    delete queued[item.chapterId];
    await send('DELETE', `/downloads/${item.chapterId}`).catch((e) => (error = message(e)));
  }

  async function retryDownload(item: UpdateItem) {
    await send('POST', `/downloads/${item.chapterId}/retry`).catch((e) => (error = message(e)));
    await loadQueue();
  }

  async function run() {
    note = '';
    try {
      const status = await send<Updates['status']>('POST', '/updates/run');
      if (data) data.status = status;
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
        <strong>Checked {ago(data.status.finishedAt)}: {data.status.found} new{data.status.failed ? `, ${data.status.failed} failed` : ''}{data.status.waiting ? `, ${data.status.waiting} not due yet` : ''}</strong>
      {/if}
      <button class="btn big spot" disabled={running} onclick={run}>
        <Icon name="refresh" size={18} stroke={3} />
        {running && data ? `Checking ${data.status.done} / ${data.status.total}` : 'Update now'}
      </button>
    </div>
  </div>

  <Notice {loading} {error} retry={load} />
  {#if inQueue || note}
    <p role="status">
      <strong>{inQueue ? `${inQueue} ${inQueue === 1 ? 'chapter' : 'chapters'} from here in the download queue${waits === 'held' ? ', on hold while storage is full' : waits === 'paused' ? ', paused' : ''}.` : note}</strong>
      <a class="link" href="#/downloads">See downloads</a>
    </p>
  {/if}

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

  {#snippet row(item: UpdateItem)}
    {@const fetch = queued[item.chapterId]}
    <li class:done={item.read}>
      <a class="main" href="#/title/{item.mangaId}">
        <strong>{item.title}</strong>
        <span>{item.chapterName}{item.sourceName ? ` · ${item.sourceName}` : ''}{item.read ? ' · read' : ''}</span>
      </a>
      <span class="tools">
        <DownloadTag saved={item.downloaded} entry={fetch} {waits} />
        {#if item.downloaded}
          <!-- Saved: deleting the file is done from the title's page. -->
        {:else if fetch?.state === 'failed'}
          <button class="btn icon" aria-label="Try downloading {item.chapterName} of {item.title} again" title="Try again" onclick={() => retryDownload(item)}><Icon name="refresh" /></button>
        {:else if fetch}
          <button class="btn icon" aria-label="Cancel the download of {item.chapterName} of {item.title}" title="Cancel the download" onclick={() => cancelDownload(item)}><Icon name="close" /></button>
        {:else}
          <button class="btn icon" aria-label="Download {item.chapterName} of {item.title}" title="Download" onclick={() => download([item])}><Icon name="download" /></button>
        {/if}
        <a class="btn" href="#/read/{item.chapterId}">Read<Icon name="arrow" size={18} stroke={3} /></a>
      </span>
    </li>
  {/snippet}

  {#each days as group (group.day)}
    <section>
      <div class="section-head"><h2 class="display">{dayLabel(group.day)}</h2></div>
      <ul class="rows">
        {#each group.titles as title (title.mangaId)}
          {@const key = `${group.day}:${title.mangaId}`}
          {#if title.items.length === 1}
            {@render row(title.items[0])}
          {:else}
            {@const first = title.items[title.items.length - 1]}
            {@const unread = title.items.filter((i) => !i.read)}
            {@const fetches = title.items.map((i) => queued[i.chapterId]).filter(Boolean)}
            <!-- The one being fetched says most; otherwise the first still waiting. -->
            {@const active = fetches.find((q) => q.state === 'downloading') ?? fetches.find((q) => q.state !== 'failed') ?? fetches[0]}
            <li class:done={unread.length === 0}>
              <a class="main" href="#/title/{title.mangaId}">
                <strong>{first.title}</strong>
                <span>{title.items.length} new chapters{unread.length && unread.length < title.items.length ? `, ${unread.length} unread` : ''}{first.sourceName ? ` · ${first.sourceName}` : ''}</span>
              </a>
              <span class="tools">
                <DownloadTag saved={title.items.every((i) => i.downloaded)} entry={active} {waits} />
                {#if title.items.some((i) => !i.downloaded && !queued[i.chapterId])}
                  <button class="btn icon" aria-label="Download the new chapters of {first.title}" title="Download the new chapters" onclick={() => download(title.items)}><Icon name="download" /></button>
                {/if}
                <button class="btn icon" aria-label={opened[key] ? 'Hide the chapters' : 'Show the chapters'} aria-expanded={!!opened[key]} onclick={() => (opened[key] = !opened[key])}>
                  <Icon name={opened[key] ? 'up' : 'down'} />
                </button>
                <a class="btn" href="#/read/{(unread.at(-1) ?? first).chapterId}">Read<Icon name="arrow" size={18} stroke={3} /></a>
              </span>
            </li>
            {#if opened[key]}
              {#each title.items as item (item.chapterId)}{@render row(item)}{/each}
            {/if}
          {/if}
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
