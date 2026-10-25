<script lang="ts">
  import { ApiError, get, message, send } from '../lib/api';
  import { every } from '../lib/util';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';

  type Source = { id: string; name: string; titles: number; installed: boolean; pkg: string | null; storeId: number | null; extension: string | null };
  type Preview = { titles: number; inLibrary: number; chapters: number; readChapters: number; tracked: number; categories: string[]; sources: Source[]; unreadable: number };
  type Report = {
    titlesAdded: number;
    titlesMerged: number;
    chaptersAdded: number;
    chaptersUpdated: number;
    categoriesAdded: number;
    historyEntries: number;
    trackerLinks: number;
    skipped: number;
    reasons: string[];
    sources: Source[];
  };
  type Status = { running: boolean; done: number; total: number; report: Report | null; error: string | null };

  let file = $state<File | null>(null);
  let preview = $state<Preview | null>(null);
  let status = $state<Status | null>(null);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let installing = $state<Record<string, boolean>>({});

  const report = $derived(status?.report ?? null);
  const sources = $derived(report?.sources ?? preview?.sources ?? []);
  const missing = $derived(sources.filter((s) => !s.installed));

  /** The file goes up as it is, not as JSON. */
  async function upload<T>(path: string): Promise<T> {
    const response = await fetch('api' + path, { method: 'POST', body: file });
    const answer = await response.json().catch(() => null);
    if (!response.ok) throw new ApiError(response.status, answer?.error?.code ?? 'error', answer?.error?.message ?? `Mango answered ${response.status}`);
    return answer as T;
  }

  async function choose(event: Event) {
    file = (event.currentTarget as HTMLInputElement).files?.[0] ?? null;
    preview = null;
    status = null;
    error = null;
    if (!file) return;
    busy = true;
    try {
      preview = await upload<Preview>('/import/preview');
    } catch (e) {
      error = message(e);
    }
    busy = false;
  }

  async function start() {
    error = null;
    try {
      status = await upload<Status>('/import');
    } catch (e) {
      error = message(e);
    }
  }

  // While the import runs, the screen follows it.
  $effect(() => {
    if (!status?.running) return;
    const stop = every(700, async () => (status = await get<Status>('/import').catch(() => status)));
    return stop;
  });

  async function install(source: Source) {
    if (!source.pkg) return;
    installing[source.id] = true;
    error = null;
    try {
      await send('POST', `/extensions/${source.pkg}/install${source.storeId ? `?store=${source.storeId}` : ''}`);
      for (const other of sources) if (other.pkg === source.pkg) other.installed = true;
    } catch (e) {
      error = message(e);
    }
    installing[source.id] = false;
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display">IMPORT</h1>
    <a class="btn" href="#/settings"><Icon name="back" size={16} stroke={3} />Settings</a>
  </div>

  <p class="lead">
    Bring a library over from Mihon or Suwayomi. Make a backup there, then pick the <code>.tachibk</code> file here. Titles, categories, read
    chapters, bookmarks, history and tracker links come across. Nothing you already have in Mango is overwritten: a chapter read on either side
    stays read.
  </p>

  <label class="btn big pick">
    <Icon name="plus" size={18} stroke={3} />{file ? file.name : 'Choose a backup file'}
    <input class="sr" type="file" accept=".tachibk,.gz,application/gzip,application/octet-stream" onchange={choose} />
  </label>

  <Notice loading={busy} {error} />

  {#if preview && !status}
    <section class="panel box">
      <h2 class="display">IN THIS BACKUP</h2>
      <ul class="facts">
        <li><strong>{preview.titles}</strong> {preview.titles === 1 ? 'title' : 'titles'}, {preview.inLibrary} of them in the library</li>
        <li><strong>{preview.chapters}</strong> {preview.chapters === 1 ? 'chapter' : 'chapters'}, {preview.readChapters} read</li>
        <li><strong>{preview.categories.length}</strong> {preview.categories.length === 1 ? 'category' : 'categories'}{preview.categories.length ? `: ${preview.categories.join(', ')}` : ''}</li>
        <li><strong>{preview.tracked}</strong> titles with tracker links</li>
        {#if preview.unreadable}<li><strong>{preview.unreadable}</strong> entries that cannot be read and will be skipped</li>{/if}
      </ul>
      <button class="btn big spot" onclick={start}>Import<Icon name="arrow" size={18} stroke={3} /></button>
    </section>
  {/if}

  {#if status?.running}
    <section class="panel box" role="status">
      <h2 class="display">IMPORTING</h2>
      <span class="meter"><span style:width="{status.total ? Math.round((status.done / status.total) * 100) : 0}%"></span></span>
      <strong>{status.done} of {status.total} titles</strong>
    </section>
  {:else if status?.error}
    <Notice error="The import stopped and nothing was changed: {status.error}" />
  {:else if report}
    <section class="panel box">
      <h2 class="display">DONE</h2>
      <ul class="facts">
        <li><strong>{report.titlesAdded}</strong> {report.titlesAdded === 1 ? 'title' : 'titles'} added, <strong>{report.titlesMerged}</strong> merged with ones you had</li>
        <li><strong>{report.chaptersAdded}</strong> {report.chaptersAdded === 1 ? 'chapter' : 'chapters'} added, <strong>{report.chaptersUpdated}</strong> moved forward</li>
        <li><strong>{report.categoriesAdded}</strong> {report.categoriesAdded === 1 ? 'category' : 'categories'} added, <strong>{report.historyEntries}</strong> history entries, <strong>{report.trackerLinks}</strong> tracker links</li>
        {#if report.skipped}<li><strong>{report.skipped}</strong> skipped</li>{/if}
      </ul>
      {#if report.reasons.length}
        <details>
          <summary class="btn">Why they were skipped</summary>
          <ul class="facts">
            {#each report.reasons as reason, n (n)}<li>{reason}</li>{/each}
          </ul>
        </details>
      {/if}
      <a class="btn big spot" href="#/">Open the library<Icon name="arrow" size={18} stroke={3} /></a>
    </section>
  {/if}

  {#if missing.length}
    <section>
      <div class="section-head"><h2 class="display">EXTENSIONS NEEDED</h2></div>
      <p class="muted">These titles are imported all the same. They can be opened and read again once their extension is installed.</p>
      <ul class="rows">
        {#each missing as source (source.id)}
          <li>
            <span class="main">
              <strong>{source.name}</strong>
              <span>{source.titles} {source.titles === 1 ? 'title' : 'titles'}{source.extension ? ` · ${source.extension}` : ' · no added store has its extension'}</span>
            </span>
            {#if source.pkg}
              <button class="btn" disabled={installing[source.id]} onclick={() => install(source)}>{installing[source.id] ? 'Installing…' : 'Install'}</button>
            {:else}
              <a class="btn" href="#/extensions">Extensions</a>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .lead {
    max-width: 720px;
  }

  .pick {
    align-self: flex-start;
    cursor: pointer;
  }

  .box {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 14px;
    max-width: 720px;
    padding: 24px;
  }

  .box h2 {
    font-size: 22px;
  }

  .box .meter {
    align-self: stretch;
  }

  .box .meter span {
    background: var(--spot);
  }

  .facts {
    margin: 0;
    padding-left: 20px;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .rows {
    margin: 0;
    padding: 0;
    list-style: none;
  }
</style>
