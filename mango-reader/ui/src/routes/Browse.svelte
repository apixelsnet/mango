<script lang="ts">
  import { get, message, send, type Source } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { browseLangs, setPref } from '../lib/prefs.svelte';
  import { langName } from '../lib/util';

  let sources = $state<Source[]>([]);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let showHidden = $state(false);
  let picking = $state(false);

  const langs = $derived([...new Set(sources.filter((s) => s.id !== '0').map((s) => s.lang))].sort());
  const chosen = $derived(browseLangs(sources));
  // The local source has no language to pick, so the language choice never hides it.
  const shown = $derived(
    sources.filter((s) => (showHidden || !s.hidden) && (s.pinned || s.id === '0' || chosen.length === 0 || chosen.includes(s.lang))),
  );
  const pinned = $derived(shown.filter((s) => s.pinned));
  const others = $derived(shown.filter((s) => !s.pinned));
  const hiddenCount = $derived(sources.filter((s) => s.hidden).length);

  async function load() {
    loading = true;
    error = null;
    try {
      sources = (await get<{ sources: Source[] }>('/sources')).sources;
    } catch (e) {
      error = message(e);
    }
    loading = false;
  }
  load();

  async function change(source: Source, patch: { pinned?: boolean; hidden?: boolean }) {
    Object.assign(source, patch);
    try {
      await send('PUT', `/sources/${source.id}`, patch);
    } catch (e) {
      error = message(e);
    }
  }

  function toggleLang(lang: string) {
    setPref('browse.langs', chosen.includes(lang) ? chosen.filter((l) => l !== lang) : [...chosen, lang]);
  }
</script>

{#snippet row(source: Source)}
  <li>
    <a class="main" href="#/source/{source.id}">
      <strong>{source.name}</strong>
      <span>{source.id === '0' ? 'Files in your local folder' : langName(source.lang)}{source.hidden ? ' · hidden' : ''}</span>
    </a>
    <span class="tools">
      <button
        class="btn icon spot-on"
        aria-pressed={source.pinned}
        aria-label={source.pinned ? `Unpin ${source.name}` : `Pin ${source.name}`}
        onclick={() => change(source, { pinned: !source.pinned })}
      >
        <Icon name="pin" fill={source.pinned} />
      </button>
      <button
        class="btn icon"
        aria-pressed={source.hidden}
        aria-label={source.hidden ? `Show ${source.name}` : `Hide ${source.name}`}
        onclick={() => change(source, { hidden: !source.hidden })}
      >
        <Icon name={source.hidden ? 'eyeoff' : 'eye'} />
      </button>
    </span>
  </li>
{/snippet}

<div class="page">
  <div class="page-head">
    <h1 class="display">BROWSE</h1>
    <div class="tools">
      {#if langs.length > 1}
        <button class="btn icon" aria-label="Languages" title="Languages" aria-expanded={picking} onclick={() => (picking = !picking)}>
          <Icon name="sliders" size={18} />
        </button>
      {/if}
      <a class="btn" href="#/extensions"><Icon name="puzzle" size={18} />Extensions</a>
    </div>
  </div>

  <Notice {loading} {error} retry={load} />

  {#if !loading && !error}
    {#if sources.every((s) => s.id === '0')}
      <Notice>
        No online sources yet. Add a store and install an extension to get some.
        <a class="btn spot" href="#/extensions">Open extensions</a>
      </Notice>
    {/if}
    {#if sources.length}
      {#if picking && langs.length > 1}
        <div class="tools" role="group" aria-label="Languages">
          <button class="btn pill" aria-pressed={chosen.length === 0} onclick={() => setPref('browse.langs', [])}>All</button>
          {#each langs as lang (lang)}
            <button class="btn pill" aria-pressed={chosen.includes(lang)} onclick={() => toggleLang(lang)}>{langName(lang)}</button>
          {/each}
        </div>
      {/if}

      {#if pinned.length}
        <section>
          <div class="section-head"><h2 class="display">PINNED</h2></div>
          <ul class="rows">
            {#each pinned as source (source.id)}{@render row(source)}{/each}
          </ul>
        </section>
      {/if}

      <section>
        <div class="section-head">
          <h2 class="display">SOURCES</h2>
          {#if hiddenCount}
            <button class="btn" aria-pressed={showHidden} onclick={() => (showHidden = !showHidden)}>
              <Icon name="eyeoff" size={18} />Hidden ({hiddenCount})
            </button>
          {/if}
        </div>
        <ul class="rows">
          {#each others as source (source.id)}{@render row(source)}{/each}
        </ul>
        {#if others.length === 0 && pinned.length === 0}
          <Notice>No source matches the languages picked.</Notice>
        {/if}
      </section>
    {/if}
  {/if}
</div>

<style>
  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }
</style>
