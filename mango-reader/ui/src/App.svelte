<script lang="ts">
  import { get } from './lib/api';
  import { display } from './lib/display.svelte';
  import Icon, { type IconName } from './lib/Icon.svelte';
  import Notice from './lib/Notice.svelte';
  import Tour from './lib/Tour.svelte';
  import { tour } from './lib/guide.svelte';
  import { back, go, route } from './lib/router.svelte';
  import { loadSession, session } from './lib/session.svelte';
  import { loadVersion, version } from './lib/version.svelte';
  import SignIn from './routes/SignIn.svelte';
  import Downloads from './routes/Downloads.svelte';
  import UpdatesScreen from './routes/Updates.svelte';
  import Browse from './routes/Browse.svelte';
  import History from './routes/History.svelte';
  import Library from './routes/Library.svelte';
  import Search from './routes/Search.svelte';
  import Source from './routes/Source.svelte';
  import Title from './routes/Title.svelte';

  // Screens not every visit opens are fetched when first opened, so the first paint waits for less.
  const later = {
    reader: () => import('./routes/Reader.svelte'),
    settings: () => import('./routes/Settings.svelte'),
    extensions: () => import('./routes/Extensions.svelte'),
    extension: () => import('./routes/Extension.svelte'),
    import: () => import('./routes/Import.svelte'),
    migrate: () => import('./routes/Migrate.svelte'),
    migrateMany: () => import('./routes/MigrateMany.svelte'),
    stats: () => import('./routes/Stats.svelte'),
    welcome: () => import('./routes/Welcome.svelte'),
  };

  // `under` lists the first path segments that light the entry up.
  const NAV: { to: string; label: string; icon: IconName; under: string[] }[] = [
    { to: '/', label: 'Library', icon: 'book', under: [''] },
    { to: '/updates', label: 'Updates', icon: 'refresh', under: ['updates'] },
    { to: '/browse', label: 'Browse', icon: 'globe', under: ['browse', 'source', 'search', 'extensions', 'extension'] },
    { to: '/downloads', label: 'Downloads', icon: 'download', under: ['downloads'] },
    { to: '/history', label: 'History', icon: 'clock', under: ['history'] },
  ];

  const parts = $derived(route.path.split('/').filter(Boolean));
  const area = $derived(parts[0] ?? '');
  // On a narrow screen the menu is a row of tabs along the bottom, under the thumb.
  const tabbed = $derived(display.tabs);
  // There a title is a screen of its own: it brings its own way back, and the menu makes room for its buttons.
  const pushed = $derived(tabbed && area === 'title');
  // Any other screen that is not one of the menu's gets a way back: an app on a phone's home screen has no other.
  const deep = $derived(tabbed && !NAV.some((item) => item.to === route.path));
  // Unread chapters found lately, shown beside Updates. Looked up again on each change of screen.
  let fresh = $state(0);

  $effect(() => {
    void area;
    if (!session.signedIn) return;
    get<{ fresh: number }>('/updates/fresh')
      .then((updates) => (fresh = updates.fresh))
      .catch(() => {});
  });

  // Whether a newer Mango is out: wanted for a mark on the settings button, never waited for.
  $effect(() => {
    if (session.signedIn) loadVersion().catch(() => {});
  });
  const news = $derived(version.info?.updateAvailable ?? false);

  // An invitation's link has done its work once its holder is in.
  $effect(() => {
    if (session.signedIn && area === 'join') go('/', true);
  });

  /** A press on the tab already open goes back to its top, as in a phone's own apps. */
  function again(event: MouseEvent, to: string) {
    if (route.path !== to) return;
    event.preventDefault();
    scrollTo({ top: 0, behavior: 'smooth' });
  }
</script>

{#snippet badge()}
  {#if fresh > 0}<span class="badge"><span class="sr">New chapters: </span>{fresh > 99 ? '99+' : fresh}</span>{/if}
{/snippet}

{#if !session.known}
  {#if session.error}
    <div class="page"><Notice error={session.error} retry={loadSession} /></div>
  {/if}
{:else if !session.signedIn}
  {#if session.signup}
    {#await later.welcome() then { default: Welcome }}<Welcome />{/await}
  {:else}
    <SignIn />
  {/if}
{:else if area === 'read' && parts[1]}
  {#await later.reader() then { default: Reader }}
    {#key parts[1]}
      <Reader id={Number(parts[1])} />
    {/key}
  {/await}
{:else}
  <div class="shell" class:tabbed={tabbed && !pushed}>
    {#if !pushed}
      <header class="top">
        {#if deep}
          <button class="btn icon" aria-label="Back" onclick={() => back()}><Icon name="back" stroke={3} /></button>
        {/if}
        <a class="logo" class:alone={deep} href="#/" aria-label="Mango home">
          <span class="dot"></span>
          <span class="display word">MANGO</span>
          <span class="kana" aria-hidden="true">マンゴー</span>
        </a>
        {#if display.value.incognito}
          <a class="tag ink incognito" href="#/settings/appearance" title="Reading here leaves no trace. Turn it off in Settings.">Incognito</a>
        {/if}
        {#if !tabbed}
          <nav aria-label="Primary">
            {#each NAV as item (item.to)}
              <a href="#{item.to}" aria-current={item.under.includes(area) ? 'page' : undefined}>
                {item.label}
                {#if item.to === '/updates'}{@render badge()}{/if}
              </a>
            {/each}
          </nav>
        {/if}
        <!-- Only where whoever runs this Mango keeps accounts: the account tab of Settings, which leads to their page. -->
        {#if session.account}
          <a class="btn icon account" href="#/settings/account" aria-label="Your account" title="Your account" aria-current={area === 'settings' && parts[1] === 'account' ? 'page' : undefined}><Icon name="user" /></a>
        {/if}
        <a class="btn icon settings" href="#/settings" aria-label={news ? 'Settings: a newer Mango is out' : 'Settings'} aria-current={area === 'settings' && !(session.account && parts[1] === 'account') ? 'page' : undefined}>
          <Icon name="sliders" />
          {#if news}<span class="news" aria-hidden="true"></span>{/if}
        </a>
      </header>
    {/if}

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
        {#await later.settings() then { default: Settings }}<Settings />{/await}
      {:else if area === 'import'}
        {#await later.import() then { default: Import }}<Import />{/await}
      {:else if area === 'stats'}
        {#await later.stats() then { default: Stats }}<Stats />{/await}
      {:else if area === 'search'}
        {#key route.query.toString()}
          <Search q={route.query.get('q') ?? ''} from={Number(route.query.get('for')) || null} merge={Number(route.query.get('merge')) || null} />
        {/key}
      {:else if area === 'migrate' && parts[1] === 'many'}
        {#await later.migrateMany() then { default: MigrateMany }}
          {#key route.query.toString()}
            <MigrateMany ids={(route.query.get('ids') ?? '').split(',').map(Number).filter(Boolean)} />
          {/key}
        {/await}
      {:else if area === 'migrate' && parts[1] && parts[2]}
        {#await later.migrate() then { default: Migrate }}
          {#key route.path}
            <Migrate from={Number(parts[1])} to={Number(parts[2])} />
          {/key}
        {/await}
      {:else if area === 'source' && parts[1]}
        {#key parts[1]}
          <Source id={parts[1]} />
        {/key}
      {:else if area === 'extensions'}
        {#await later.extensions() then { default: Extensions }}<Extensions />{/await}
      {:else if area === 'extension' && parts[1]}
        {#await later.extension() then { default: Extension }}
          {#key parts[1]}
            <Extension pkg={parts[1]} />
          {/key}
        {/await}
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

    {#if tabbed && !pushed}
      <nav class="tabs" aria-label="Primary">
        {#each NAV as item (item.to)}
          <a href="#{item.to}" aria-current={item.under.includes(area) ? 'page' : undefined} onclick={(e) => again(e, item.to)}>
            <Icon name={item.icon} size={22} />
            <span class="label">{item.label}</span>
            {#if item.to === '/updates'}{@render badge()}{/if}
          </a>
        {/each}
      </nav>
    {/if}
  </div>
{/if}

{#if tour.open && session.signedIn && area !== 'read'}
  <Tour />
{/if}

<style>
  /* Everything but the reader. The size picked for this device scales it whole. */
  .shell {
    zoom: var(--zoom);
    padding: 0 var(--safe-right) 0 var(--safe-left);
  }

  .shell.tabbed {
    --tabs: calc(59px + var(--safe-bottom));
  }

  main {
    padding-bottom: var(--tabs);
  }

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
    flex: none;
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

  .incognito {
    flex: none;
  }

  /* The menu stays on one line and scrolls sideways if it must. */
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

  /* Against the far edge, whatever stands before it. */
  .settings {
    position: relative;
    margin-left: auto;
  }

  /* With the account's button there, it takes the push to the right, and settings sits beside it. */
  .account {
    margin-left: auto;
  }

  .account + .settings {
    margin-left: 0;
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
  }

  /* ---------- the menu along the bottom ---------- */

  /* With it there, the top is one line: the way back or the name, and the settings. */
  .tabbed .top {
    flex-wrap: nowrap;
    gap: 10px;
    padding: 10px var(--gutter);
  }

  .tabbed .logo {
    gap: 8px;
  }

  .tabbed .dot {
    width: 26px;
    height: 26px;
  }

  .tabbed .word {
    font-size: 20px;
  }

  .tabbed .kana,
  .tabbed .alone .word {
    display: none;
  }

  @media (max-width: 359px) {
    .tabbed .word {
      display: none;
    }
  }

  .tabs {
    position: fixed;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 6;
    display: grid;
    grid-auto-columns: minmax(0, 1fr);
    grid-auto-flow: column;
    gap: 0;
    padding: 0 var(--safe-right) var(--safe-bottom) var(--safe-left);
    overflow: visible;
    border-top: 3px solid var(--ink);
    background: var(--paper);
    user-select: none;
    -webkit-touch-callout: none;
  }

  .tabs a {
    position: relative;
    flex-direction: column;
    justify-content: center;
    gap: 3px;
    min-height: 56px;
    padding: 0 2px;
    /* The names keep to their tabs: on a small screen at a large size they stop growing. */
    font-size: min(10px, calc(2.6vw / var(--zoom)));
    letter-spacing: 0.04em;
  }

  .tabs a:focus-visible {
    outline-offset: -5px;
  }

  .label {
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .tabs .badge {
    position: absolute;
    top: 3px;
    left: calc(50% + 4px);
    min-width: 20px;
    height: 20px;
    padding: 0 5px;
    font-size: 11px;
  }
</style>
