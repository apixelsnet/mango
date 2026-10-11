<script lang="ts">
  import { get, message, send, type Category, type Chapter, type Title } from '../lib/api';
  import Cover from '../lib/Cover.svelte';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import Tracking from '../lib/Tracking.svelte';
  import { ago, chapterShort, langName, STATUS } from '../lib/util';

  const WEEK = 7 * 86400;

  let { id }: { id: number } = $props();

  let title = $state<Title | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  // A failed refresh keeps what is on screen and says so, instead of replacing it.
  let refreshing = $state(false);
  let refreshError = $state<string | null>(null);
  let unreadOnly = $state(false);
  let menu = $state(false);
  let wholeText = $state(false);

  const chapters = $derived(title?.chapters ?? []);
  const unread = $derived(chapters.filter((c) => !c.read).length);
  const saved = $derived(chapters.filter((c) => c.downloaded).length);
  const listed = $derived(unreadOnly ? chapters.filter((c) => !c.read) : chapters);
  // Reading goes from the bottom of the list up: the next one to read is the last unread.
  const resume = $derived(chapters.findLast((c) => !c.read) ?? null);
  const started = $derived(chapters.some((c) => c.read || c.lastPageRead > 0));

  async function load() {
    loading = true;
    error = null;
    try {
      title = await get<Title>(`/manga/${id}`);
      // A title only seen in a list has no details or chapters yet.
      if (!title.initialized || title.chapters.length === 0) refresh();
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  async function refresh() {
    menu = false;
    refreshing = true;
    refreshError = null;
    try {
      title = await send<Title>('POST', `/manga/${id}/refresh`);
    } catch (e) {
      refreshError = message(e);
    }
    refreshing = false;
  }

  let allCategories = $state<Category[]>([]);
  get<{ categories: Category[] }>('/categories')
    .then((answer) => (allCategories = answer.categories))
    .catch(() => {});

  function toggleCategory(category: number) {
    if (!title) return;
    const t = title;
    t.categories = t.categories.includes(category) ? t.categories.filter((c) => c !== category) : [...t.categories, category];
    change(() => send('PUT', `/manga/${id}/categories`, { ids: t.categories }), true);
  }

  let note = $state('');

  function download(list: Chapter[]) {
    change(async () => {
      const answer = await send<{ queued: number }>('POST', '/downloads', { chapters: list.map((c) => c.id) });
      note = answer.queued ? `${answer.queued} ${answer.queued === 1 ? 'chapter' : 'chapters'} queued for download.` : 'Already saved or queued.';
    });
  }

  function deleteFile(chapter: Chapter) {
    chapter.downloaded = false;
    change(() => send('DELETE', `/chapters/${chapter.id}/download`));
  }

  function toggleAuto() {
    if (!title) return;
    const t = title;
    t.autoDownload = !t.autoDownload;
    change(() => send('PUT', `/manga/${id}/auto-download`, { on: t.autoDownload }), true);
  }

  async function change(work: () => Promise<unknown>, keepMenu = false) {
    menu = keepMenu && menu;
    refreshError = null;
    try {
      await work();
    } catch (e) {
      refreshError = message(e);
      title = await get<Title>(`/manga/${id}`).catch(() => title);
    }
  }

  function toggleLibrary() {
    if (!title) return;
    const t = title;
    t.inLibrary = !t.inLibrary;
    change(() => send('PUT', `/manga/${id}/library`, { inLibrary: t.inLibrary }));
  }

  function mark(list: Chapter[], read: boolean) {
    for (const chapter of list) {
      chapter.read = read;
      chapter.lastPageRead = 0;
    }
    change(() => send('PUT', `/manga/${id}/chapters`, { ids: list.map((c) => c.id), read }));
  }

  function about(chapter: Chapter): string {
    const when = chapter.dateUpload > 0 ? ago(chapter.dateUpload / 1000) : chapter.fetchedAt > 0 ? `found ${ago(chapter.fetchedAt)}` : '';
    const where = chapter.read
      ? 'read'
      : chapter.lastPageRead > 0
        ? `page ${chapter.lastPageRead + 1}${chapter.pageCount ? ` of ${chapter.pageCount}` : ''}`
        : chapter.pageCount
          ? `${chapter.pageCount} pages`
          : '';
    return [when, chapter.scanlator, where, chapter.downloaded ? 'saved' : ''].filter(Boolean).join(' · ');
  }

  function isNew(chapter: Chapter): boolean {
    return !chapter.read && chapter.lastPageRead === 0 && !!title?.addedAt && chapter.fetchedAt > title.addedAt && Date.now() / 1000 - chapter.fetchedAt < WEEK;
  }
</script>

<div class="page">
  <Notice {loading} {error} retry={load} />

  {#if title}
    <div class="bar">
      <a class="btn icon lift" href="#/" aria-label="Back" onclick={(e) => history.length > 1 && (e.preventDefault(), history.back())}>
        <Icon name="back" stroke={3} />
      </a>
      <div class="tools">
        <button
          class="btn lift spot-on"
          aria-pressed={title.inLibrary}
          onclick={toggleLibrary}
        >
          <Icon name="bookmark" fill={title.inLibrary} />{title.inLibrary ? 'In library' : 'Add to library'}
        </button>
        <div class="menu">
          <button class="btn icon lift" aria-label="More actions" aria-expanded={menu} onclick={() => (menu = !menu)}><Icon name="more" fill /></button>
          {#if menu}
            <div class="panel sheet">
              <button class="btn" onclick={refresh}><Icon name="refresh" size={18} />Refresh from the source</button>
              <button class="btn" onclick={() => mark(chapters, true)}><Icon name="check" size={18} />Mark all read</button>
              <button class="btn" onclick={() => mark(chapters, false)}>Mark all unread</button>
              <a class="btn" href="#/search?q={encodeURIComponent(title.title)}&for={id}"><Icon name="arrow" size={18} />Move to another source</a>
              {#if title.inLibrary}
                <button class="btn" aria-pressed={title.autoDownload} onclick={toggleAuto}>
                  {#if title.autoDownload}<Icon name="check" size={18} stroke={3} />{/if}Download new chapters
                </button>
              {/if}
              {#if title.inLibrary && allCategories.length}
                <div class="cats" role="group" aria-label="Categories">
                  <span>Categories</span>
                  {#each allCategories as category (category.id)}
                    {@const inIt = title.categories.includes(category.id)}
                    <button class="btn" aria-pressed={inIt} onclick={() => toggleCategory(category.id)}>
                      {#if inIt}<Icon name="check" size={18} stroke={3} />{/if}{category.name}
                    </button>
                  {/each}
                </div>
              {/if}
            </div>
          {/if}
        </div>
      </div>
    </div>

    <div class="columns">
      <div class="about">
        <div class="hero">
          <div class="art"><Cover src={title.cover} title={title.title} tilt /></div>
          <div class="facts">
            <div class="tools">
              <span class="tag ink">{STATUS[title.status] ?? 'Unknown'}</span>
              {#if title.sourceLang}<span class="tag">{langName(title.sourceLang)}</span>{/if}
            </div>
            <h1 class="display">{title.title}</h1>
            <strong>{title.sourceName ?? 'Its extension is not installed'}</strong>
            {#if title.author || title.artist}
              <span>{[...new Set([title.author, title.artist].filter(Boolean))].join(' · ')}</span>
            {/if}
          </div>
        </div>

        <div class="stats">
          <div><span class="display">{chapters.length}</span><span>Chapters</span></div>
          <div class:hot={unread > 0}><span class="display">{unread}</span><span>Unread</span></div>
          <div><span class="display">{saved}</span><span>Saved</span></div>
        </div>

        {#if title.description}
          <p class="text" class:clamp={!wholeText}>{title.description}</p>
          {#if title.description.length > 280}
            <button class="btn more" aria-expanded={wholeText} onclick={() => (wholeText = !wholeText)}>{wholeText ? 'Less' : 'More'}</button>
          {/if}
        {/if}
        {#if title.genres.length}
          <div class="tools genres">
            {#each title.genres as genre (genre)}<span class="tag">{genre}</span>{/each}
          </div>
        {/if}
        <Tracking {id} title={title.title} />
      </div>

      <div class="chapters">
        <div class="section-head">
          <h2 class="display">CHAPTERS</h2>
          <div class="tools">
            <button class="btn" aria-pressed={unreadOnly} onclick={() => (unreadOnly = !unreadOnly)}><Icon name="filter" size={18} />Unread</button>
            <button
              class="btn icon"
              aria-label="Download the unread chapters"
              disabled={!chapters.some((c) => !c.read && !c.downloaded)}
              onclick={() => download(chapters.filter((c) => !c.read && !c.downloaded))}
            >
              <Icon name="download" />
            </button>
            <button class="btn icon" aria-label="Refresh from the source" disabled={refreshing} onclick={refresh}><Icon name="refresh" /></button>
          </div>
        </div>

        <Notice loading={refreshing} error={refreshError} retry={refresh} />
        {#if note}<p role="status"><strong>{note}</strong> <a class="link" href="#/downloads">See downloads</a></p>{/if}

        <ul class="rows">
          {#each listed as chapter (chapter.id)}
            <li class:done={chapter.read}>
              <a class="main" href="#/read/{chapter.id}">
                <strong>{chapter.name}</strong>
                <span>{about(chapter)}</span>
              </a>
              {#if isNew(chapter)}
                <span class="tag spot">New</span>
              {:else if !chapter.read && chapter.lastPageRead > 0 && chapter.pageCount}
                <span class="meter" aria-hidden="true"><span style:width="{Math.round(((chapter.lastPageRead + 1) / chapter.pageCount) * 100)}%"></span></span>
              {/if}
              {#if chapter.downloaded}
                <button class="btn icon" aria-label="Delete the saved file of {chapter.name}" onclick={() => deleteFile(chapter)}><Icon name="trash" /></button>
              {:else}
                <button class="btn icon" aria-label="Download {chapter.name}" onclick={() => download([chapter])}><Icon name="download" /></button>
              {/if}
              <button
                class="btn icon"
                aria-pressed={chapter.read}
                aria-label={chapter.read ? `Mark ${chapter.name} unread` : `Mark ${chapter.name} read`}
                onclick={() => mark([chapter], !chapter.read)}
              >
                <Icon name="check" stroke={3} />
              </button>
            </li>
          {/each}
        </ul>
        {#if !refreshing && listed.length === 0}
          <Notice>{chapters.length ? 'Every chapter is read.' : 'No chapters yet.'}</Notice>
        {/if}
      </div>
    </div>

    <div class="dock">
      {#if resume}
        <a class="btn big spot" href="#/read/{resume.id}">
          {started ? 'Resume' : 'Start'} {chapterShort(resume)}<Icon name="arrow" size={18} stroke={3} />
        </a>
      {:else if chapters.length}
        <a class="btn big" href="#/read/{chapters[chapters.length - 1].id}">Read again<Icon name="arrow" size={18} stroke={3} /></a>
      {/if}
      {#if title.webUrl}
        <a class="btn big icon" href={title.webUrl} target="_blank" rel="noreferrer noopener" aria-label="Open on the source's site">
          <Icon name="globe" />
        </a>
      {/if}
    </div>
  {/if}
</div>

<style>
  .page {
    gap: 20px;
    padding-bottom: 120px;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }

  .menu {
    position: relative;
  }

  .sheet {
    position: absolute;
    top: 52px;
    right: 0;
    z-index: 5;
    display: flex;
    flex-direction: column;
    min-width: 240px;
  }

  .sheet .btn {
    justify-content: flex-start;
    border-width: 0 0 2px;
  }

  .sheet .btn:last-child {
    border-bottom: 0;
  }

  .link {
    text-decoration: underline;
  }

  .cats {
    display: flex;
    flex-direction: column;
  }

  .cats span {
    padding: 10px 14px 6px;
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  .columns {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 28px 48px;
  }

  @media (min-width: 960px) {
    .columns {
      grid-template-columns: minmax(0, 5fr) minmax(0, 7fr);
      align-items: start;
    }
  }

  .about {
    display: flex;
    flex-direction: column;
    gap: 18px;
  }

  .hero {
    display: flex;
    gap: 22px;
    padding: 6px 4px 0;
  }

  .art {
    flex: 0 0 clamp(110px, 30%, 170px);
  }

  .facts {
    display: flex;
    flex-direction: column;
    gap: 10px;
    min-width: 0;
  }

  .facts h1 {
    font-size: clamp(24px, 4.4vw, 40px);
    overflow-wrap: anywhere;
  }

  .facts strong,
  .facts > span {
    font-size: 13px;
    font-weight: 700;
  }

  .stats {
    display: grid;
    grid-template-columns: repeat(3, minmax(0, 1fr));
    border: 3px solid var(--ink);
    background: var(--surface);
  }

  .stats div {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 10px 4px;
    border-right: 3px solid var(--ink);
  }

  .stats div:last-child {
    border-right: 0;
  }

  .stats .hot {
    background: var(--spot);
    color: var(--on-spot);
  }

  .stats .display {
    font-size: 22px;
    line-height: 1.15;
  }

  .stats span:last-child {
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.1em;
    text-transform: uppercase;
  }

  .text {
    font-size: 14px;
    line-height: 1.5;
    white-space: pre-line;
    overflow-wrap: anywhere;
  }

  .clamp {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 5;
    line-clamp: 5;
  }

  .more {
    align-self: flex-start;
  }

  .genres .tag {
    letter-spacing: 0.06em;
  }

  .chapters {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-width: 0;
  }

  .chapters h2 {
    font-size: 20px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .done .main {
    color: var(--muted);
  }

  .done .main strong {
    font-weight: 700;
  }

  .meter {
    flex: 0 0 56px;
  }

  .dock {
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    display: flex;
    justify-content: center;
    gap: 14px;
    padding: 14px var(--gutter) calc(14px + env(safe-area-inset-bottom));
    border-top: 3px solid var(--ink);
    background: var(--paper);
  }

  .dock .btn:first-child {
    flex: 0 1 420px;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
