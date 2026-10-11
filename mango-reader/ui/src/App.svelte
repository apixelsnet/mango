<script lang="ts">
  import { get, type Updates } from './lib/api';
  import Icon from './lib/Icon.svelte';
  import Notice from './lib/Notice.svelte';
  import { go, route } from './lib/router.svelte';
  import { loadSession, session } from './lib/session.svelte';
  import { loadVersion, version } from './lib/version.svelte';
  import SignIn from './routes/SignIn.svelte';
  import Downloads from './routes/Downloads.svelte';
  import UpdatesScreen from './routes/Updates.svelte';
  import Browse from './routes/Browse.svelte';
  import Extension from './routes/Extension.svelte';
  import Extensions from './routes/Extensions.svelte';
  import History from './routes/History.svelte';
  import Import from './routes/Import.svelte';
  import Library from './routes/Library.svelte';
  import Migrate from './routes/Migrate.svelte';
  import Reader from './routes/Reader.svelte';
  import Search from './routes/Search.svelte';
  import Settings from './routes/Settings.svelte';
  import Source from './routes/Source.svelte';
  import Title from './routes/Title.svelte';

  // `under` lists the first path segments that light the entry up.
  const NAV = [
    { to: '/', label: 'Library', under: [''] },
    { to: '/updates', label: 'Updates', under: ['updates'] },
    { to: '/browse', label: 'Browse', under: ['browse', 'source', 'search', 'extensions', 'extension'] },
    { to: '/downloads', label: 'Downloads', under: ['downloads'] },
    { to: '/history', label: 'History', under: ['history'] },
  ];

  const parts = $derived(route.path.split('/').filter(Boolean));
  const area = $derived(parts[0] ?? '');
  let query = $state('');
  // Unread chapters found lately, shown beside Updates. Looked up again on each change of screen.
  let fresh = $state(0);

  $effect(() => {
    void area;
    if (!session.signedIn) return;
    get<Updates>('/updates?limit=1')
      .then((updates) => (fresh = updates.fresh))
      .catch(() => {});
  });

  // Whether a newer Mango is out: wanted for a mark on the settings button, never waited for.
  $effect(() => {
    if (session.signedIn) loadVersion().catch(() => {});
  });
  const news = $derived(version.info?.updateAvailable ?? false);

  function search(event: SubmitEvent) {
    event.preventDefault();
    const text = query.trim();
    if (text) go('/search?q=' + encodeURIComponent(text));
  }
</script>

{#if !session.known}
  {#if session.error}
    <div class="page"><Notice error={session.error} retry={loadSession} /></div>
  {/if}
{:else if !session.signedIn}
  <SignIn />
{:else if area === 'read' && parts[1]}
  {#key parts[1]}
    <Reader id={Number(parts[1])} />
  {/key}
{:else}
  <header class="top">
    <a class="logo" href="#/" aria-label="Mango home">
      <span class="dot"></span>
      <span class="display word">MANGO</span>
      <span class="kana" aria-hidden="true">マンゴー</span>
    </a>
    <nav aria-label="Primary">
      {#each NAV as item (item.to)}
        <a href="#{item.to}" aria-current={item.under.includes(area) ? 'page' : undefined}>
          {item.label}
          {#if item.to === '/updates' && fresh > 0}<span class="badge"><span class="sr">New chapters: </span>{fresh > 99 ? '99+' : fresh}</span>{/if}
        </a>
      {/each}
    </nav>
    <form class="field" role="search" onsubmit={search}>
      <Icon name="search" size={18} stroke={3} />
      <input type="search" bind:value={query} placeholder="Search all sources" aria-label="Search all sources" />
    </form>
    <a class="btn icon settings" href="#/settings" aria-label={news ? 'Settings: a newer Mango is out' : 'Settings'} aria-current={area === 'settings' ? 'page' : undefined}>
      <Icon name="sliders" />
      {#if news}<span class="news" aria-hidden="true"></span>{/if}
    </a>
  </header>

  <main>
    {#if area === ''}
      <Library />
    {:else if area === 'browse'}
      <Browse />
    {:else if area === 'updates'}
      <UpdatesScreen />
    {:else if area === 'downloads'}
      <Downloads />
    {:else if area === 'history'}
      <History />
    {:else if area === 'settings'}
      <Settings />
    {:else if area === 'import'}
      <Import />
    {:else if area === 'search'}
      {#key route.query.toString()}
        <Search q={route.query.get('q') ?? ''} from={Number(route.query.get('for')) || null} />
      {/key}
    {:else if area === 'migrate' && parts[1] && parts[2]}
      {#key route.path}
        <Migrate from={Number(parts[1])} to={Number(parts[2])} />
      {/key}
    {:else if area === 'source' && parts[1]}
      {#key parts[1]}
        <Source id={parts[1]} />
      {/key}
    {:else if area === 'extensions'}
      <Extensions />
    {:else if area === 'extension' && parts[1]}
      {#key parts[1]}
        <Extension pkg={parts[1]} />
      {/key}
    {:else if area === 'title' && parts[1]}
      {#key parts[1]}
        <Title id={Number(parts[1])} />
      {/key}
    {:else}
      <div class="page">
        <h1 class="display">Nothing here</h1>
        <p><a class="btn" href="#/">Back to the start</a></p>
      </div>
    {/if}
  </main>
{/if}

<style>
  .top {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px 28px;
    padding: 14px var(--gutter);
    border-bottom: 3px solid var(--ink);
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 44px;
  }

  .dot {
    width: 30px;
    height: 30px;
    border: 3px solid var(--ink);
    border-radius: 50%;
    background: var(--spot);
  }

  .word {
    font-size: 28px;
    line-height: 1;
  }

  .kana {
    padding: 3px 6px;
    border: 2px solid var(--ink);
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.18em;
  }

  /* The menu stays on one line and scrolls sideways if it must; the search gives way first. */
  nav {
    display: flex;
    flex: 1 1 auto;
    gap: 4px;
    min-width: 0;
    overflow-x: auto;
  }

  nav a {
    display: flex;
    flex: none;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 14px;
    font-size: 13px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  nav a[aria-current='page'] {
    background: var(--ink);
    color: var(--paper);
  }

  form {
    flex: 0 1 260px;
    min-width: 150px;
  }

  .settings {
    position: relative;
  }

  .news {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 14px;
    height: 14px;
    border: 2px solid var(--ink);
    border-radius: 50%;
    background: var(--spot);
  }

  @media (max-width: 640px) {
    .kana {
      display: none;
    }

    nav {
      order: 3;
      flex-basis: 100%;
      flex-wrap: nowrap;
      overflow-x: auto;
    }

    form {
      flex: 1 1 160px;
    }
  }
</style>
