<script lang="ts">
  import { ApiError, message } from '../lib/api';
  import Notice from '../lib/Notice.svelte';
  import { session, signIn } from '../lib/session.svelte';

  const SHORTEST = 8;

  // On the main page the form stands alone: that page has the logo and the frame.
  let { inside = false }: { inside?: boolean } = $props();

  let name = $state('');
  let password = $state('');
  let again = $state('');
  let code = $state('');
  /** The password was right and the account asks for a code from its authenticator too. */
  let second = $state(false);
  let otp = $state('');
  let error = $state<string | null>(null);
  let busy = $state(false);

  const first = $derived(session.setup);
  // One account signs in with its password alone, as Mango always did.
  const named = $derived(session.named && !first);

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    error = null;
    if (first && password !== again) {
      error = 'The two passwords are not the same.';
      return;
    }
    busy = true;
    try {
      await signIn(password, first, named ? name : '', first ? code : otp);
    } catch (e) {
      if (e instanceof ApiError && e.code === 'code_required') {
        second = true;
      } else {
        error = message(e);
      }
      busy = false;
    }
  }
</script>

<svelte:element this={inside ? 'div' : 'main'} class:door={!inside}>
  <form class="panel" onsubmit={submit}>
    {#if !inside}
      <div class="logo">
        <span class="dot"></span>
        <span class="display word">MANGO</span>
      </div>
    {/if}
    <svelte:element this={inside ? 'h2' : 'h1'} class="display title">{first ? 'SET A PASSWORD' : 'SIGN IN'}</svelte:element>
    {#if first}
      <p>This Mango is new. Pick the password that will open it from now on: {SHORTEST} characters or more.</p>
    {/if}

    {#if named}
      <label class="field">
        <span class="sr">Name</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input type="text" name="username" bind:value={name} placeholder="Name" autocomplete="username" autocapitalize="none" required autofocus={!inside} />
      </label>
    {:else}
      <!-- Password managers file the password under this name. -->
      <input class="sr" type="text" name="username" autocomplete="username" value="mango" tabindex="-1" aria-hidden="true" readonly />
    {/if}
    <label class="field">
      <span class="sr">Password</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        type="password"
        name="password"
        bind:value={password}
        placeholder="Password"
        autocomplete={first ? 'new-password' : 'current-password'}
        minlength={first ? SHORTEST : undefined}
        required
        autofocus={!named && !inside}
      />
    </label>
    {#if first}
      <label class="field">
        <span class="sr">Confirm password</span>
        <input type="password" bind:value={again} placeholder="Confirm password" autocomplete="new-password" required />
      </label>
    {/if}
    {#if first && session.setupCode}
      <p>
        It also takes the setup code, to show this is your Mango: it is in Mango's log, and in the file
        <code>setup-code</code> in its data folder.
      </p>
      <label class="field">
        <span class="sr">Setup code</span>
        <input type="text" bind:value={code} placeholder="Setup code" autocomplete="off" autocapitalize="none" spellcheck="false" required />
      </label>
    {/if}

    {#if second}
      <p>Enter the six digits your authenticator app shows for Mango, or one of your recovery codes.</p>
      <label class="field">
        <span class="sr">Code</span>
        <!-- svelte-ignore a11y_autofocus -->
        <input type="text" bind:value={otp} placeholder="Code" inputmode="numeric" autocomplete="one-time-code" autocapitalize="none" spellcheck="false" required autofocus />
      </label>
    {/if}

    <Notice {error} />
    <button class="btn big ink" disabled={busy}>{first ? 'Set it and enter' : 'Sign in'}</button>
  </form>
</svelte:element>

<style>
  .door {
    display: grid;
    place-items: center;
    min-height: calc(100dvh - env(safe-area-inset-top, 0px));
    padding: 24px var(--gutter);
  }

  form {
    display: flex;
    flex-direction: column;
    gap: 16px;
    width: min(420px, 100%);
    padding: 28px;
  }

  .field {
    box-shadow: none;
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

  .title {
    font-size: 30px;
  }
</style>
