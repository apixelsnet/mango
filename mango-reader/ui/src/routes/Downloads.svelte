<script lang="ts">
  import { get, message, send, type Queue, type Queued } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { session } from '../lib/session.svelte';
  import { ago, every } from '../lib/util';

  let data = $state<Queue | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let sure = $state(false);

  const queue = $derived(data?.queue ?? []);
  // Only chapters still waiting can be moved; the one being fetched stays where it is.
  const movable = $derived(queue.filter((q) => q.state !== 'downloading'));

  async function load() {
    try {
      data = await get<Queue>('/downloads');
      error = null;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  // The queue changes by itself: chapters finish, and updates add new ones. So the
  // screen looks again every two seconds while it is open and in view.
  $effect(() => every(2000, load));

  async function act(method: string, path: string, body?: unknown) {
    try {
      await send(method, path, body);
    } catch (e) {
      error = message(e);
    }
    await load();
  }

  function move(entry: Queued, by: number) {
    const ids = movable.map((q) => q.chapterId);
    const at = ids.indexOf(entry.chapterId);
    ids.splice(at, 1);
    ids.splice(at + by, 0, entry.chapterId);
    act('PUT', '/downloads/order', { ids });
  }

  function clear() {
    if (!sure) {
      sure = true;
      return;
    }
    sure = false;
    act('DELETE', '/downloads');
  }

  function describe(entry: Queued): string {
    if (entry.state === 'downloading') return entry.total ? `Downloading, page ${entry.done} of ${entry.total}` : 'Downloading';
    if (entry.state === 'failed') return `Gave up after ${entry.attempts} tries: ${entry.error ?? 'unknown error'}`;
    if (entry.error) {
      const wait = entry.retryAt ? Math.max(0, Math.ceil((entry.retryAt - Date.now() / 1000) / 60)) : 0;
      return `Try ${entry.attempts} failed: ${entry.error}. Next try ${wait > 0 ? `in ${wait} min` : 'soon'}`;
    }
    return data?.held ? 'Waiting, storage full' : data?.paused ? 'Waiting, queue paused' : 'Waiting';
  }
</script>

<div class="page">
  <div class="page-head">
    <div class="heading">
      <h1 class="display">DOWNLOADS</h1>
      <strong>{queue.length} in the queue{data?.paused ? ', paused' : ''}</strong>
    </div>
    {#if data}
      <div class="tools">
        {#if data.paused}
          <button class="btn big spot" onclick={() => act('POST', '/downloads/resume')}><Icon name="play" size={18} fill />Resume</button>
        {:else}
          <button class="btn big" onclick={() => act('POST', '/downloads/pause')}><Icon name="pause" size={18} stroke={3} />Pause</button>
        {/if}
        {#if queue.some((q) => q.state === 'failed')}
          <button class="btn" onclick={() => act('POST', '/downloads/retry')}><Icon name="refresh" size={18} />Retry all that failed</button>
        {/if}
        {#if queue.length}
          <button class="btn" onclick={clear}><Icon name="trash" size={18} />{sure ? 'Really clear the queue?' : 'Clear queue'}</button>
        {/if}
      </div>
    {/if}
  </div>

  <Notice {loading} {error} retry={load} />

  {#if data?.held}
    <Notice>
      On hold: your storage is full. The queue waits, and goes on by itself once some downloads are deleted{session.account ? ' or the plan grows' : ''}.
      {#if session.account}<a class="btn" href="#/settings/downloads">Free up space</a>{/if}
    </Notice>
  {/if}

  {#if data}
    {#if queue.length === 0}
      <Notice>
        Nothing is waiting. Chapters are queued from a title's page, from Updates, or by themselves for titles set to download new chapters.
      </Notice>
    {:else}
      <ul class="rows">
        {#each queue as entry (entry.chapterId)}
          {@const at = movable.indexOf(entry)}
          <li>
            <a class="main" href="#/title/{entry.mangaId}">
              <strong>{entry.title}</strong>
              <span>{entry.chapterName} · {describe(entry)}</span>
              {#if entry.state === 'downloading' && entry.total}
                <span class="meter" aria-hidden="true"><span style:width="{Math.round((entry.done / entry.total) * 100)}%"></span></span>
              {/if}
            </a>
            <span class="tools">
              {#if entry.state === 'failed'}
                <button class="btn" onclick={() => act('POST', `/downloads/${entry.chapterId}/retry`)}><Icon name="refresh" size={18} />Try again</button>
              {/if}
              {#if at >= 0}
                <button class="btn icon" aria-label="Move {entry.chapterName} up" disabled={at === 0} onclick={() => move(entry, -1)}><Icon name="up" /></button>
                <button class="btn icon" aria-label="Move {entry.chapterName} down" disabled={at === movable.length - 1} onclick={() => move(entry, 1)}><Icon name="down" /></button>
              {/if}
              <button class="btn icon" aria-label="Cancel {entry.chapterName} of {entry.title}" onclick={() => act('DELETE', `/downloads/${entry.chapterId}`)}>
                <Icon name="close" />
              </button>
            </span>
          </li>
        {/each}
      </ul>
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

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    flex-wrap: wrap;
  }

  .meter {
    width: min(320px, 100%);
    margin-top: 6px;
  }

  .meter span {
    background: var(--spot);
  }
</style>
