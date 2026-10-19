<script lang="ts">
  import { onMount, tick } from 'svelte';
  import Icon, { type IconName } from './Icon.svelte';
  import { session } from './session.svelte';
  import { display } from './display.svelte';
  import { go, route } from './router.svelte';
  import { endTour, tour } from './guide.svelte';

  // Each card opens the screen it is about, so the reader sees the place while it is
  // explained; one with no screen stays where the last one was.
  type Card = { icon: IconName; title: string; text: string; to?: string; where?: string; link?: { href: string; label: string } };

  // The account's own card comes only where whoever runs this Mango keeps accounts.
  const cards = $derived<Card[]>([
    {
      icon: 'book',
      title: 'Welcome to Mango',
      text: 'A reader for manga, manhwa and comics. This tour walks you through the app in a minute: press Next and it takes you to each place. Skip it whenever you like.',
      to: '/',
      where: 'Library',
    },
    {
      icon: 'puzzle',
      title: 'Add a source',
      text: 'Titles come from sources, and sources are extensions. Install one here. If no store is listed yet, add a store’s address first.',
      to: '/extensions',
      where: 'Extensions',
    },
    {
      icon: 'globe',
      title: 'Browse and search',
      text: 'Every source you install is listed under Browse. Search all of them at once, or open one to see what is popular and new.',
      to: '/browse',
      where: 'Browse',
    },
    {
      icon: 'bookmark',
      title: 'Keep a library',
      text: 'Add a title to your library from its page and it lives here. Categories and filters keep a large one in order.',
      to: '/',
      where: 'Library',
    },
    {
      icon: 'refresh',
      title: 'New chapters find you',
      text: 'Your library is checked for new chapters on a schedule. They are listed here, with what you have not read yet counted.',
      to: '/updates',
      where: 'Updates',
    },
    {
      icon: 'eye',
      title: 'Read your way',
      text: 'In a chapter, tap the sides of a page to turn it and the middle for the controls. Each kind of series has its own direction and fit. The first chapter you open shows where the tap zones are.',
    },
    {
      icon: 'download',
      title: 'Keep it, take it with you',
      text: 'Download chapters to read when a source is slow or gone; they wait here. A backup to and from Mihon or Suwayomi is under Settings.',
      to: '/downloads',
      where: 'Downloads',
    },
    ...(session.account
      ? [
          {
            icon: 'user',
            title: 'Your account',
            text: 'Your plan, the room your downloads take, billing and your email are on your account page. Settings, Account links to it too.',
            to: '/settings',
            where: 'Settings',
            link: { href: session.account, label: 'Open your account' },
          } satisfies Card,
        ]
      : []),
    {
      icon: 'sliders',
      title: 'That is all',
      text: 'Themes, colours and the reader’s defaults are here in Settings. This tour is too, under About, if you want it again.',
      to: '/settings',
      where: 'Settings',
    },
  ]);

  const at = $derived(Math.min(tour.step, cards.length - 1));
  const card = $derived(cards[at]);
  const last = $derived(at >= cards.length - 1);
  let panel = $state<HTMLElement>();

  /** Opens the screen a card is about, unless it is open already. */
  function visit(step: number) {
    const to = cards[Math.min(step, cards.length - 1)].to;
    if (to && route.path !== to) go(to);
  }

  async function move(by: number) {
    tour.step = Math.max(0, Math.min(at + by, cards.length - 1));
    visit(tour.step);
    // The new card is read out from its heading.
    await tick();
    panel?.querySelector<HTMLElement>('h2')?.focus();
  }

  // A tour started again from Settings begins at the library. Back from a chapter mid-tour,
  // the card comes back where the reader is.
  onMount(() => {
    if (tour.step === 0) visit(0);
  });

  function key(event: KeyboardEvent) {
    if (event.key === 'Escape' && panel?.contains(document.activeElement)) endTour();
  }
</script>

<svelte:window onkeydown={key} />

<!-- Not modal: the screen behind it is the point, and stays usable. -->
<div bind:this={panel} class="tour" class:tabbed={display.tabs} role="dialog" aria-modal="false" aria-labelledby="tour-title">
  <div class="head">
    <span class="mark"><Icon name={card.icon} size={22} stroke={2.5} /></span>
    <span class="step">{at + 1} / {cards.length}{#if card.where}<span class="where">{' · '}{card.where}</span>{/if}</span>
    <button class="btn icon" aria-label="Close the tour" onclick={endTour}><Icon name="close" /></button>
  </div>
  <h2 id="tour-title" class="display" tabindex="-1">{card.title}</h2>
  <p>{card.text}</p>
  {#if card.link}
    <p><a class="link" href={card.link.href} target="_blank" rel="noopener">{card.link.label}<Icon name="arrow" size={16} stroke={3} /></a></p>
  {/if}
  <div class="bar" aria-hidden="true"><span style:width="{((at + 1) / cards.length) * 100}%"></span></div>
  <div class="foot">
    {#if at > 0}
      <button class="btn" onclick={() => move(-1)}><Icon name="back" size={16} stroke={3} />Back</button>
    {:else}
      <button class="btn" onclick={endTour}>Skip</button>
    {/if}
    {#if last}
      <button class="btn spot" onclick={endTour}>Start reading</button>
    {:else}
      <button class="btn ink" onclick={() => move(1)}>Next<Icon name="arrow" size={16} stroke={3} /></button>
    {/if}
  </div>
</div>

<style>
  .tour {
    position: fixed;
    right: calc(20px + var(--safe-right));
    bottom: calc(20px + var(--safe-bottom));
    z-index: 30;
    width: min(380px, calc(100vw - 32px));
    max-height: calc(100dvh - 40px);
    padding: 18px;
    overflow: auto;
    border: 3px solid var(--ink);
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--sh-8);
    animation: rise 0.18s ease-out;
  }

  /* Above the row of tabs, across the screen. */
  .tour.tabbed {
    right: calc(12px + var(--safe-right));
    left: calc(12px + var(--safe-left));
    bottom: calc(84px + var(--safe-bottom));
    width: auto;
    max-height: calc(100dvh - 120px);
  }

  @keyframes rise {
    from { transform: translateY(12px); opacity: 0; }
  }

  @media (prefers-reduced-motion: reduce) {
    .tour { animation: none; }
  }

  .head {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 40px;
    height: 40px;
    border: 3px solid var(--ink);
    border-radius: 50%;
    background: var(--spot);
    color: var(--on-spot);
  }

  .step {
    margin-right: auto;
    font-size: 14px;
    font-weight: 900;
    letter-spacing: 0.06em;
  }

  .where { color: var(--muted); }

  h2 {
    margin: 14px 0 6px;
    font-size: 22px;
    line-height: 1.15;
    outline: none;
  }

  p {
    margin: 0 0 12px;
    line-height: 1.5;
  }

  .link {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-weight: 900;
    text-decoration: underline;
    text-underline-offset: 3px;
  }

  .bar {
    height: 6px;
    margin: 4px 0 14px;
    border: 2px solid var(--ink);
  }

  .bar span {
    display: block;
    height: 100%;
    background: var(--spot);
    transition: width 0.2s;
  }

  .foot {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }
</style>
