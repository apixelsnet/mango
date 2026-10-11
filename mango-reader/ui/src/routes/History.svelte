<script lang="ts">
  import { get, message, send, type HistoryEntry } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { dayKey, dayLabel, whenVisible } from '../lib/util';

  const PAGE = 100;

  let entries = $state<HistoryEntry[]>([]);
  let more = $state(false);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let sure = $state(false);

  // Entries arrive newest first, so each new day starts a new group.
  const days = $derived.by(() => {
    const groups: { day: number; entries: HistoryEntry[] }[] = [];
    for (const entry of entries) {
      const day = dayKey(entry.readAt);
      if (groups.at(-1)?.day === day) groups.at(-1)!.entries.push(entry);
      else groups.push({ day, entries: [entry] });
    }
    return groups;
  });

  async function load(fresh: boolean) {
    loading = true;
    error = null;
    try {
      const page = (await get<{ entries: HistoryEntry[] }>(`/history?limit=${PAGE}&offset=${fresh ? 0 : entries.length}`)).entries;
      entries = fresh ? page : [...entries, ...page];
      more = page.length === PAGE;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load(true);

  async function forget(entry: HistoryEntry) {
    entries = entries.filter((e) => e.chapterId !== entry.chapterId);
    try {
      await send('DELETE', `/history/${entry.chapterId}`);
    } catch (e) {
      error = message(e);
      load(true);
    }
  }

  async function clear() {
    if (!sure) {
      sure = true;
      return;
    }
    sure = false;
    try {
      await send('DELETE', '/history');
      entries = [];
      more = false;
    } catch (e) {
      error = message(e);
    }
  }

  const time = (seconds: number) => new Date(seconds * 1000).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });

  function place(entry: HistoryEntry): string {
    if (entry.read) return 'finished';
    return `page ${entry.page + 1}${entry.pageCount ? ` of ${entry.pageCount}` : ''}`;
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display">HISTORY</h1>
    {#if entries.length}
      <button class="btn" onclick={clear}><Icon name="trash" size={18} />{sure ? 'Really clear all of it?' : 'Clear history'}</button>
    {/if}
  </div>

  {#each days as group (group.day)}
    <section>
      <div class="section-head"><h2 class="display">{dayLabel(group.day)}</h2></div>
      <ul class="rows">
        {#each group.entries as entry (entry.chapterId)}
          <li>
            <a class="main" href="#/title/{entry.mangaId}">
              <strong>{entry.title}</strong>
              <span>{entry.chapterName} · {place(entry)} · {time(entry.readAt)}</span>
            </a>
            <span class="tools">
              <a class="btn" href="#/read/{entry.chapterId}">{entry.read ? 'Read again' : 'Resume'}<Icon name="arrow" size={18} stroke={3} /></a>
              <button class="btn icon" aria-label="Remove {entry.chapterName} of {entry.title} from the history" onclick={() => forget(entry)}>
                <Icon name="close" />
              </button>
            </span>
          </li>
        {/each}
      </ul>
    </section>
  {/each}

  <Notice {loading} {error} retry={() => load(true)} />
  {#if !loading && !error && entries.length === 0}
    <Notice>Nothing read yet. Chapters show up here once you open them.</Notice>
  {/if}
  {#if more && !loading}
    <div class="more" use:whenVisible={() => more && !loading && load(false)}>
      <button class="btn" onclick={() => load(false)}>Load more</button>
    </div>
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

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    flex-wrap: wrap;
  }

  .more {
    display: flex;
    justify-content: center;
  }
</style>
