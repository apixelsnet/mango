<script lang="ts">
  import { get, message, send, type Track, type Tracker, type TrackerMatch, type TrackingView } from './api';
  import Icon from './Icon.svelte';
  import Locked from './Locked.svelte';
  import Notice from './Notice.svelte';
  import { lacks } from './plan.svelte';

  // A title's links to trackers. Read once the page is up, so the title never waits for
  // it; and no tracker is asked anything until something here is pressed.
  let { id, title }: { id: number; title: string } = $props();

  let trackers = $state<Tracker[]>([]);
  let tracks = $state<Track[]>([]);
  let error = $state<string | null>(null);
  let busy = $state<number | null>(null);

  // A tracker shows when someone is signed in to it, or a backup brought a link to it.
  const shown = $derived(trackers.filter((t) => t.signedIn || tracks.some((link) => link.tracker === t.id)));
  const linkOf = (t: Tracker) => tracks.find((link) => link.tracker === t.id);
  const path = (t: Tracker) => `/manga/${id}/tracks/${t.id}`;

  function load() {
    get<TrackingView>(`/manga/${id}/tracks`)
      .then((answer) => ((trackers = answer.trackers), (tracks = answer.tracks)))
      .catch(() => {});
  }
  load();

  /** Runs a change to one link. `null` back means the link is gone. */
  async function act(t: Tracker, work: () => Promise<Track | null>) {
    busy = t.id;
    error = null;
    try {
      const track = await work();
      tracks = [...tracks.filter((other) => other.tracker !== t.id), ...(track ? [track] : [])];
    } catch (e) {
      error = message(e);
      // The screen may show what was not saved.
      tracks = await get<TrackingView>(`/manga/${id}/tracks`)
        .then((answer) => answer.tracks)
        .catch(() => tracks);
    }
    busy = null;
  }

  const edit = (t: Tracker, change: Partial<Pick<Track, 'status' | 'score' | 'lastChapterRead'>>) => act(t, () => send<Track>('PUT', path(t), change));
  const refresh = (t: Tracker) => act(t, () => send<Track>('POST', path(t) + '/refresh'));
  const unlink = (t: Tracker) => act(t, () => send('DELETE', path(t)).then(() => null));

  // ---------- finding the title on a tracker ----------

  let finding = $state<Tracker | null>(null);
  let query = $state('');
  let matches = $state<TrackerMatch[] | null>(null);
  let searching = $state(false);
  let findError = $state<string | null>(null);

  function open(t: Tracker) {
    finding = t;
    query = title;
    matches = null;
    search();
  }

  async function search(event?: SubmitEvent) {
    event?.preventDefault();
    const t = finding;
    if (!t || !query.trim()) return;
    searching = true;
    findError = null;
    try {
      const answer = await get<{ results: TrackerMatch[] }>(`/trackers/${t.id}/search?q=${encodeURIComponent(query.trim())}`);
      if (finding?.id === t.id) matches = answer.results;
    } catch (e) {
      findError = message(e);
    }
    searching = false;
  }

  function pick(match: TrackerMatch) {
    const t = finding;
    if (!t) return;
    finding = null;
    act(t, () => send<Track>('POST', path(t), match));
  }

  const typed = (event: Event) => Number((event.currentTarget as HTMLInputElement).value);
  // A link's address came from a tracker or from a backup file: only a web address is shown as one.
  const isLink = (url: string) => /^https:\/\//.test(url);
  const facts = (match: TrackerMatch) => [match.kind, match.year, match.totalChapters ? `${match.totalChapters} ${match.totalChapters === 1 ? 'chapter' : 'chapters'}` : ''].filter(Boolean).join(' · ');
</script>

{#if shown.length}
  <section class="tracking" aria-label="Tracking">
    <h2 class="display">TRACKING</h2>
    <Notice {error} />
    <ul class="rows">
      {#each shown as t (t.id)}
        {@const link = linkOf(t)}
        <li>
          <div class="main">
            <strong>{t.name}</strong>
            {#if !link}
              <span class="muted">Not linked</span>
            {:else if isLink(link.url)}
              <a href={link.url} target="_blank" rel="noreferrer noopener">{link.title || 'Linked'}</a>
            {:else}
              <span>{link.title || 'Linked'}</span>
            {/if}
            {#if link?.error}<span class="late" role="status">Not sent yet: {link.error}</span>{/if}
            {#if link && !t.signedIn}
              <span class="muted">Sign in to {t.name} in <a href="#/settings/trackers">Settings</a> to keep it in step.</span>
            {/if}
          </div>
          {#if !link && lacks('trackers')}
            <Locked />
          {:else if !link}
            <button class="btn" disabled={busy === t.id} onclick={() => open(t)}><Icon name="link" size={18} />Link</button>
          {:else}
            <div class="tools">
              {#if t.signedIn && !lacks('trackers')}
                <select
                  class="btn"
                  aria-label="Status on {t.name}"
                  value={link.status}
                  disabled={busy === t.id}
                  onchange={(e) => edit(t, { status: Number(e.currentTarget.value) })}
                >
                  {#each t.statuses as status (status.value)}<option value={status.value}>{status.label}</option>{/each}
                </select>
                <label class="count">
                  <span>Ch.</span>
                  <input
                    type="number"
                    min="0"
                    step="1"
                    aria-label="Chapters read, on {t.name}"
                    value={link.lastChapterRead}
                    disabled={busy === t.id}
                    onchange={(e) => edit(t, { lastChapterRead: typed(e) })}
                  />
                  {#if link.totalChapters}<span>of {link.totalChapters}</span>{/if}
                </label>
                <label class="count">
                  <span>Score</span>
                  <input
                    type="number"
                    min="0"
                    max="10"
                    step={t.scoreStep}
                    placeholder="–"
                    aria-label="Score out of ten, on {t.name}"
                    value={link.score ? link.score / t.scoreScale : ''}
                    disabled={busy === t.id}
                    onchange={(e) => edit(t, { score: Math.round(typed(e) * t.scoreScale * 10) / 10 })}
                  />
                </label>
                <button class="btn icon" aria-label="Compare with {t.name} again" disabled={busy === t.id} onclick={() => refresh(t)}>
                  <Icon name="refresh" />
                </button>
              {/if}
              <button class="btn icon" aria-label="Remove the link to {t.name}" disabled={busy === t.id} onclick={() => unlink(t)}>
                <Icon name="close" />
              </button>
            </div>
          {/if}
        </li>
      {/each}
    </ul>

    {#if finding}
      <div class="panel find">
        <div class="bar">
          <form class="field" role="search" onsubmit={search}>
            <Icon name="search" size={18} stroke={3} />
            <input type="search" bind:value={query} aria-label="Search {finding.name}" />
          </form>
          <button class="btn" onclick={() => (finding = null)}>Cancel</button>
        </div>
        <Notice loading={searching} error={findError} />
        {#if matches && !searching}
          <ul class="rows">
            {#each matches as match (match.mediaId)}
              <li>
                <button class="main pick" onclick={() => pick(match)}>
                  <strong>{match.title}</strong>
                  {#if facts(match)}<span>{facts(match)}</span>{/if}
                  {#if match.summary}<span class="summary muted">{match.summary}</span>{/if}
                </button>
              </li>
            {/each}
          </ul>
          {#if !matches.length}<p class="muted">Nothing by that name on {finding.name}.</p>{/if}
        {/if}
      </div>
    {/if}
  </section>
{/if}

<style>
  .tracking {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  h2 {
    padding-bottom: 10px;
    border-bottom: 3px solid var(--ink);
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

  .main {
    flex-basis: 140px;
  }

  .main a {
    font-size: 12.5px;
    text-decoration: underline;
    overflow-wrap: anywhere;
  }

  .late {
    font-weight: 700;
  }

  .count {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 0 10px;
    border: 2px solid var(--ink);
    background: var(--surface);
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .count input {
    width: 3.2em;
    border: 0;
    outline: 0;
    background: transparent;
    font-size: 14px;
    font-weight: 700;
  }

  .count:focus-within {
    outline: 3px solid var(--ink);
    outline-offset: 3px;
  }

  .find {
    display: flex;
    flex-direction: column;
    gap: 12px;
    padding: 14px;
  }

  .bar {
    display: flex;
    gap: 8px;
  }

  .bar .field {
    flex: 1;
    min-width: 0;
    box-shadow: none;
  }

  .find ul {
    max-height: 50vh;
    overflow-y: auto;
  }

  .pick {
    align-items: flex-start;
    padding: 4px 0;
    border: 0;
    background: none;
    text-align: left;
  }

  .summary {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }
</style>
