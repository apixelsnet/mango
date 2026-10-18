<script lang="ts">
  import Icon, { type IconName } from './Icon.svelte';
  import { session } from './session.svelte';
  import { endTour } from './guide.svelte';

  type Card = { icon: IconName; title: string; text: string; link?: { href: string; label: string; away?: boolean } };

  // The account's own card comes only where whoever runs this Mango keeps accounts.
  const cards = $derived<Card[]>([
    {
      icon: 'book',
      title: 'Welcome to Mango',
      text: 'A reader for manga, manhwa and comics. Titles come from sources, the ones you follow sit in your library, and new chapters are found for you. This takes a minute; skip it whenever you like.',
    },
    {
      icon: 'puzzle',
      title: 'Find titles',
      text: 'Sources are extensions. Under Browse, open Extensions and install one: if no store is listed yet, add a store’s address there first. Then search every source at once, or look through one.',
      link: { href: '#/extensions', label: 'Open extensions' },
    },
    {
      icon: 'bookmark',
      title: 'Keep a library',
      text: 'Add a title to the library from its page. Mango checks the library for new chapters and lists them under Updates; categories and filters keep a large one in order.',
    },
    {
      icon: 'eye',
      title: 'Read your way',
      text: 'Tap the sides of a page to turn it and the middle for the controls. Each kind of series has its own direction, fit and tap zones, and a title can keep its own. The first chapter you open shows where the zones are.',
    },
    {
      icon: 'download',
      title: 'Keep it, take it with you',
      text: 'Download chapters to read when a source is slow or gone. Under Settings, a backup moves your library to and from Mihon or Suwayomi, and your downloads come out as one zip.',
    },
    ...(session.account
      ? [
          {
            icon: 'user',
            title: 'Your account',
            text: 'Your plan, the room your downloads take, billing and the email we write to are on your account page, apart from the reader. Settings, Account links to it too.',
            link: { href: session.account, label: 'Open your account', away: true },
          } satisfies Card,
        ]
      : []),
    {
      icon: 'sliders',
      title: 'That is all',
      text: 'Themes, colours and the reader’s defaults are under Settings. This tour is there too, under About, if you want it again.',
    },
  ]);

  let at = $state(0);
  let dialog = $state<HTMLDialogElement>();
  const card = $derived(cards[Math.min(at, cards.length - 1)]);
  const last = $derived(at >= cards.length - 1);

  $effect(() => {
    if (dialog && !dialog.open) dialog.showModal();
  });
</script>

<!-- Escape closes it as Skip does: the dialog's own cancel. -->
<dialog bind:this={dialog} class="tour" aria-labelledby="tour-title" oncancel={endTour}>
  <div class="head">
    <span class="mark"><Icon name={card.icon} size={26} stroke={2.5} /></span>
    <span class="step">{at + 1} / {cards.length}</span>
    <button class="btn icon" aria-label="Skip the tour" onclick={endTour}><Icon name="close" /></button>
  </div>
  <h2 id="tour-title" class="display">{card.title}</h2>
  <p>{card.text}</p>
  {#if card.link}
    {#if card.link.away}
      <p><a class="link" href={card.link.href} target="_blank" rel="noopener">{card.link.label}<Icon name="arrow" size={16} stroke={3} /></a></p>
    {:else}
      <p><a class="link" href={card.link.href} onclick={endTour}>{card.link.label}<Icon name="arrow" size={16} stroke={3} /></a></p>
    {/if}
  {/if}
  <div class="dots" aria-hidden="true">
    {#each cards as _, i (i)}<span class:on={i === at}></span>{/each}
  </div>
  <div class="foot">
    {#if at > 0}
      <button class="btn" onclick={() => at--}><Icon name="back" size={16} stroke={3} />Back</button>
    {:else}
      <button class="btn" onclick={endTour}>Skip</button>
    {/if}
    {#if last}
      <button class="btn spot" onclick={endTour}>Start reading</button>
    {:else}
      <button class="btn ink" onclick={() => at++}>Next<Icon name="arrow" size={16} stroke={3} /></button>
    {/if}
  </div>
</dialog>

<style>
  .tour {
    width: min(460px, calc(100vw - 32px));
    max-height: calc(100dvh - 32px);
    padding: 22px;
    overflow: auto;
    border: 3px solid var(--ink);
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--sh-8);
  }

  .tour::backdrop {
    background: color-mix(in srgb, var(--ink) 55%, transparent);
  }

  .head {
    display: flex;
    align-items: center;
    gap: 12px;
  }

  .mark {
    display: grid;
    place-items: center;
    width: 48px;
    height: 48px;
    border: 3px solid var(--ink);
    border-radius: 50%;
    background: var(--spot);
    color: var(--on-spot);
  }

  .step {
    margin-right: auto;
    font-weight: 900;
    letter-spacing: 0.08em;
  }

  h2 {
    margin: 18px 0 8px;
    font-size: 26px;
    line-height: 1.1;
  }

  p {
    margin: 0 0 14px;
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

  .dots {
    display: flex;
    gap: 6px;
    margin: 6px 0 18px;
  }

  .dots span {
    width: 10px;
    height: 10px;
    border: 2px solid var(--ink);
    border-radius: 50%;
  }

  .dots .on {
    background: var(--ink);
  }

  .foot {
    display: flex;
    justify-content: space-between;
    gap: 10px;
  }
</style>
