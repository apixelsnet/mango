<script lang="ts">
  import { get, message, send, type Installed, type Source, type SourcePref } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { langName } from '../lib/util';

  let { pkg }: { pkg: string } = $props();

  let ext = $state<Installed | null>(null);
  let loading = $state(true);
  let error = $state<string | null>(null);
  // The source whose settings are open, and what it lets its user set.
  let open = $state<string | null>(null);
  let prefs = $state<SourcePref[]>([]);
  let asking = $state(false);
  let saving = $state(false);
  let trouble = $state<string | null>(null);
  // Text as it is typed, by position, until it is saved.
  let drafts = $state<Record<number, string>>({});
  // Counts the changes that failed, so the controls are drawn again from what is kept.
  let drawn = $state(0);

  const shown = $derived(prefs.filter((pref) => pref.visible));

  async function load() {
    loading = true;
    error = null;
    try {
      ext = (await get<{ extensions: Installed[] }>('/extensions')).extensions.find((e) => e.pkg === pkg) ?? null;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  async function hide(source: Source, hidden: boolean) {
    source.hidden = hidden;
    try {
      await send('PUT', `/sources/${source.id}`, { hidden });
    } catch (e) {
      source.hidden = !hidden;
      error = message(e);
    }
  }

  async function settings(source: Source) {
    if (open === source.id) {
      open = null;
      return;
    }
    open = source.id;
    prefs = [];
    drafts = {};
    trouble = null;
    asking = true;
    try {
      const got = await get<{ preferences: SourcePref[] }>(`/sources/${source.id}/preferences`);
      if (open === source.id) prefs = got.preferences;
    } catch (e) {
      if (open === source.id) trouble = message(e);
    }
    if (open === source.id) asking = false;
  }

  async function set(pref: SourcePref, value: boolean | string | string[]) {
    const id = open;
    if (!id || saving) return;
    saving = true;
    trouble = null;
    try {
      const change = { position: pref.position, key: pref.key, value };
      const got = await send<{ preferences: SourcePref[] }>('PUT', `/sources/${id}/preferences`, change);
      if (open === id) {
        prefs = got.preferences;
        delete drafts[pref.position];
      }
    } catch (e) {
      if (open === id) {
        trouble = message(e);
        drawn += 1;
      }
    }
    saving = false;
  }

  /** A list's summary may stand for the entry picked, as `%s`. */
  function summary(pref: SourcePref): string {
    const text = pref.summary ?? '';
    if (pref.type !== 'list' || !text.includes('%s')) return text;
    const at = typeof pref.value === 'string' ? (pref.entryValues ?? []).indexOf(pref.value) : -1;
    return text.replace('%s', at < 0 ? '' : (pref.entries?.[at] ?? ''));
  }

  function picked(pref: SourcePref): string[] {
    return Array.isArray(pref.value) ? pref.value : [];
  }

  function toggled(pref: SourcePref, value: string): string[] {
    const now = picked(pref);
    return now.includes(value) ? now.filter((v) => v !== value) : [...now, value];
  }

  function save(event: SubmitEvent, pref: SourcePref) {
    event.preventDefault();
    set(pref, drafts[pref.position] ?? '');
  }
</script>

<div class="page">
  <div class="page-head">
    <div>
      <a class="crumb" href="#/extensions"><Icon name="back" size={16} stroke={3} />Extensions</a>
      <h1 class="display">{ext?.name ?? 'Extension'}</h1>
      {#if ext}
        <strong>
          {ext.versionName} · {ext.sources.length} {ext.sources.length === 1 ? 'source' : 'sources'}{ext.contentWarning >= 3 ? ' · 18+' : ''}
        </strong>
      {/if}
    </div>
  </div>

  <Notice {loading} {error} retry={load} />
  {#if !loading && !error && !ext}
    <Notice>
      This extension is not installed.
      <a class="btn spot" href="#/extensions">Open extensions</a>
    </Notice>
  {/if}

  {#if ext}
    <section>
      <div class="section-head"><h2 class="display">SOURCES</h2></div>
      <ul class="rows">
        {#each ext.sources as source (source.id)}
          <li>
            <a class="main" href="#/source/{source.id}">
              <strong>{source.name}</strong>
              <span>{langName(source.lang)}{source.hidden ? ' · hidden from Browse' : ''}</span>
            </a>
            <span class="tools">
              <button
                class="btn icon"
                aria-label="Settings for {source.name}, {langName(source.lang)}"
                title="Settings"
                aria-expanded={open === source.id}
                onclick={() => settings(source)}
              >
                <Icon name="sliders" size={18} />
              </button>
              <button
                class="btn icon"
                aria-pressed={source.hidden}
                aria-label={source.hidden ? `Show ${source.name} in Browse` : `Hide ${source.name} from Browse`}
                onclick={() => hide(source, !source.hidden)}
              >
                <Icon name={source.hidden ? 'eyeoff' : 'eye'} />
              </button>
            </span>
            {#if open === source.id}
              <div class="panel prefs">
                <Notice loading={asking} error={trouble} />
                {#if !asking}
                  {#key drawn}
                    {#each shown as pref (pref.position)}
                      {@const off = !pref.enabled || saving}
                      <div class="pref">
                        <div class="what">
                          {#if pref.title}<strong>{pref.title}</strong>{/if}
                          {#if summary(pref)}<span>{summary(pref)}</span>{/if}
                          {#if pref.type === 'text' && pref.dialogMessage}<span>{pref.dialogMessage}</span>{/if}
                        </div>
                        {#if pref.type === 'switch'}
                          <button class="btn" aria-pressed={pref.value === true} aria-label={pref.title} disabled={off} onclick={() => set(pref, pref.value !== true)}>
                            {pref.value === true ? 'On' : 'Off'}
                          </button>
                        {:else if pref.type === 'list'}
                          <label>
                            <span class="sr">{pref.title}</span>
                            <select class="btn" disabled={off} value={typeof pref.value === 'string' ? pref.value : ''} onchange={(e) => set(pref, e.currentTarget.value)}>
                              {#if typeof pref.value !== 'string'}<option value="" disabled>Choose…</option>{/if}
                              {#each pref.entryValues ?? [] as value, i (i)}<option {value}>{pref.entries?.[i] ?? value}</option>{/each}
                            </select>
                          </label>
                        {:else if pref.type === 'multi'}
                          <div class="tools" role="group" aria-label={pref.title}>
                            {#each pref.entryValues ?? [] as value, i (i)}
                              <button class="btn pill" aria-pressed={picked(pref).includes(value)} disabled={off} onclick={() => set(pref, toggled(pref, value))}>
                                {pref.entries?.[i] ?? value}
                              </button>
                            {/each}
                          </div>
                        {:else if pref.type === 'text'}
                          <form class="tools" onsubmit={(e) => save(e, pref)}>
                            <label class="field">
                              <span class="sr">{pref.title}</span>
                              <input
                                value={drafts[pref.position] ?? (typeof pref.value === 'string' ? pref.value : '')}
                                oninput={(e) => (drafts[pref.position] = e.currentTarget.value)}
                                disabled={off}
                              />
                            </label>
                            <button class="btn ink" disabled={off || drafts[pref.position] === undefined}>Save</button>
                          </form>
                        {/if}
                      </div>
                    {/each}
                  {/key}
                  {#if !trouble && shown.length === 0}<p class="muted">This source has nothing to set.</p>{/if}
                {/if}
              </div>
            {/if}
          </li>
        {/each}
      </ul>
    </section>
  {/if}
</div>

<style>
  .page-head > div {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
  }

  .page-head h1 {
    font-size: clamp(30px, 6vw, 52px);
    overflow-wrap: anywhere;
  }

  .crumb {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 32px;
    font-size: 13px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    flex-wrap: wrap;
  }

  .prefs {
    display: flex;
    flex: 1 1 100%;
    flex-direction: column;
    gap: 12px;
    margin: 8px 8px 12px 0;
    padding: 16px;
  }

  .pref {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 16px;
  }

  .pref + .pref {
    padding-top: 12px;
    border-top: 2px solid var(--ink);
  }

  .what {
    display: flex;
    flex: 1 1 260px;
    flex-direction: column;
    min-width: 0;
  }

  .what strong {
    font-size: 15px;
    font-weight: 900;
    overflow-wrap: anywhere;
  }

  .what span {
    font-size: 12.5px;
    overflow-wrap: anywhere;
    white-space: pre-line;
  }

  .pref form {
    flex: 1 1 260px;
  }

  .pref .field {
    flex: 1;
    box-shadow: none;
  }

  .prefs p {
    margin: 0;
    font-weight: 700;
  }
</style>
