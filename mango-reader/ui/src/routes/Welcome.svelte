<script lang="ts">
  import { message } from '../lib/api';
  import Notice from '../lib/Notice.svelte';
  import { route } from '../lib/router.svelte';
  import { signUp } from '../lib/session.svelte';
  import SignIn from './SignIn.svelte';

  const SHORTEST = 8;

  // What the main page says Mango does: the README's list, shortened.
  const FEATURES = [
    { name: 'Every source, one shelf', text: 'Browse and search the sources that Mihon extensions bring, and keep what you follow in one library with categories.' },
    { name: 'A reader for each kind', text: 'Paged in either direction, vertical, or one long strip for webtoons. It opens at the page you left.' },
    { name: 'New chapters find you', text: 'Your library is checked on a schedule, and what you have not read is counted for you.' },
    { name: 'Read with no network', text: 'Chapters download as CBZ files and open without the source.' },
    { name: 'Progress that follows', text: 'AniList, MyAnimeList, Kitsu and MangaUpdates keep up as you read.' },
    { name: 'A shelf of your own', text: 'Each account has its own library, history and settings. Nobody else sees what you read.' },
  ];

  let mode = $state<'signin' | 'join'>('signin');
  let name = $state('');
  let password = $state('');
  let again = $state('');
  let code = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  // An invitation's link carries its code after `#/join/`. It may be opened while this page is up already.
  $effect.pre(() => {
    if (!route.path.startsWith('/join')) return;
    mode = 'join';
    code = route.path.split('/')[2] ?? '';
  });

  async function join(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    if (password !== again) {
      error = 'The two passwords are not the same.';
      return;
    }
    busy = true;
    try {
      await signUp(name, password, code);
    } catch (e) {
      error = message(e);
      busy = false;
    }
  }
</script>

<main class="welcome">
  <header class="hero">
    <div class="logo">
      <span class="dot"></span>
      <span class="display word">MANGO</span>
      <span class="kana" aria-hidden="true">マンゴー</span>
    </div>
    <h1 class="display">ONE SHELF FOR <span class="mark">EVERYTHING</span> YOU READ</h1>
    <p class="lead">Mango gathers manga and manhwa from the sources you choose into one library, and reads them in the browser on any screen.</p>
  </header>

  <section class="door" aria-label="Your account">
    <div class="tabs">
      <button class="btn" type="button" aria-pressed={mode === 'signin'} onclick={() => (mode = 'signin')}>Sign in</button>
      <button class="btn" type="button" aria-pressed={mode === 'join'} onclick={() => (mode = 'join')}>Create account</button>
    </div>
    {#if mode === 'signin'}
      <SignIn inside />
    {:else}
      <form class="panel" onsubmit={join}>
        <h2 class="display">CREATE ACCOUNT</h2>
        <p>An account takes an invitation from whoever runs this Mango. With it you get a library of your own.</p>
        <label class="field">
          <span class="sr">Name</span>
          <input type="text" name="username" bind:value={name} placeholder="Name" autocomplete="username" autocapitalize="none" maxlength="32" required />
        </label>
        <label class="field">
          <span class="sr">Password</span>
          <input
            type="password"
            name="password"
            bind:value={password}
            placeholder="Password, {SHORTEST} characters or more"
            autocomplete="new-password"
            minlength={SHORTEST}
            required
          />
        </label>
        <label class="field">
          <span class="sr">The password again</span>
          <input type="password" bind:value={again} placeholder="Once more" autocomplete="new-password" required />
        </label>
        <label class="field">
          <span class="sr">Invitation code</span>
          <input type="text" bind:value={code} placeholder="Invitation code" autocomplete="off" autocapitalize="none" spellcheck="false" required />
        </label>

        <Notice {error} />
        <button class="btn big ink" disabled={busy}>Create it and enter</button>
      </form>
    {/if}
  </section>

  <ul class="features">
    {#each FEATURES as feature (feature.name)}
      <li>
        <strong>{feature.name}</strong>
        <span>{feature.text}</span>
      </li>
    {/each}
  </ul>
</main>

<style>
  .welcome {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 32px;
    max-width: 1180px;
    margin: 0 auto;
    padding: 28px var(--gutter) 72px;
  }

  /* Wide: what Mango does on the left, the door beside it the whole way down. */
  @media (min-width: 900px) {
    .welcome {
      grid-template-columns: minmax(0, 1fr) 420px;
      grid-template-areas:
        'hero door'
        'features door';
      column-gap: 56px;
      align-items: start;
      padding-top: 48px;
    }

    .hero {
      grid-area: hero;
    }

    .door {
      grid-area: door;
    }

    .features {
      grid-area: features;
    }
  }

  .hero {
    display: flex;
    flex-direction: column;
    gap: 20px;
  }

  .logo {
    display: flex;
    align-items: center;
    gap: 12px;
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

  h1 {
    font-size: clamp(38px, 7vw, 72px);
    line-height: 1.12;
  }

  /* A block of its own, so the colour stays within its line and off the one above. */
  .mark {
    display: inline-block;
    padding: 0 0.12em;
    background: var(--spot);
    color: var(--on-spot);
    line-height: 1;
  }

  .lead {
    max-width: 52ch;
    font-size: 18px;
    font-weight: 700;
  }

  .door {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: 100%;
    max-width: 420px;
  }

  .tabs {
    display: flex;
    gap: 8px;
  }

  .tabs .btn {
    flex: 1;
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 16px;
    padding: 28px;
  }

  .field {
    box-shadow: none;
  }

  h2 {
    font-size: 30px;
  }

  .features {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(min(260px, 100%), 1fr));
    column-gap: 32px;
    margin: 0;
    padding: 0;
    border-top: 3px solid var(--ink);
    list-style: none;
  }

  .features li {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 14px 0;
    border-bottom: 2px solid var(--ink);
  }

  .features strong {
    font-size: 16px;
    font-weight: 900;
  }
</style>
