<script lang="ts">
  // Said when a hosted reader's storage is used up: new downloads wait for room. Offers the
  // two ways out, a plan with more room or fewer files, and closes to keep reading.
  import { onMount } from 'svelte';
  import { get, message, send, type Orphan, type ReadFiles } from './api';
  import Icon from './Icon.svelte';
  import { loadPlan, plan, sized } from './plan.svelte';
  import { go } from './router.svelte';
  import { session } from './session.svelte';
  import Storage from './Storage.svelte';

  let dialog: HTMLDialogElement;
  let read = $state<ReadFiles | null>(null);
  let cached = $state(0);
  let left = $state<Orphan[]>([]);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let freed = $state(0);

  const leftBytes = $derived(left.reduce((sum, o) => sum + o.bytes, 0));

  onMount(() => {
    dialog.showModal();
    get<ReadFiles>('/downloads/read')
      .then((found) => (read = found))
      .catch(() => {});
    get<Orphan[]>('/downloads/orphans')
      .then((found) => (left = found))
      .catch(() => {});
    get<{ bytes: number }>('/cache')
      .then((found) => (cached = found.bytes))
      .catch(() => {});
  });

  function close() {
    plan.asking = false;
  }

  async function free(path: string, bytes: number) {
    busy = true;
    error = null;
    try {
      await send('DELETE', path);
      freed += bytes;
      if (path.endsWith('/read')) read = { chapters: 0, bytes: 0 };
      else if (path === '/cache') cached = 0;
      else left = [];
      loadPlan(true);
    } catch (e) {
      error = message(e);
    }
    busy = false;
  }

  function manage() {
    dialog.close();
    go('/settings/downloads');
  }
</script>

<dialog bind:this={dialog} onclose={close} aria-labelledby="full-title">
  <div class="head">
    <span class="mark"><Icon name="download" size={22} stroke={2.5} /></span>
    <span class="kicker">{plan.is?.name ?? 'Your'} plan</span>
    <button class="btn icon" aria-label="Close" onclick={() => dialog.close()}><Icon name="close" /></button>
  </div>
  <h2 id="full-title" class="display">Storage full</h2>
  <p>Downloads, imports and browsing sources wait until there is room. Your library, and the chapters in it, still open.</p>
  {#if plan.is?.storage}<Storage storage={plan.is.storage} full />{/if}

  <div class="ways">
    {#if session.account}
      <a class="btn spot way" href={session.account}>
        <Icon name="up" size={18} stroke={3} />
        <span><strong>Get more room</strong><small>Move to a larger plan on your account page.</small></span>
      </a>
    {/if}
    {#if read?.chapters}
      <button class="btn way" disabled={busy} onclick={() => free('/downloads/read', read?.bytes ?? 0)}>
        <Icon name="trash" size={18} />
        <span><strong>Delete chapters already read</strong><small>{read.chapters} saved {read.chapters === 1 ? 'chapter' : 'chapters'}, {sized(read.bytes)}. Their progress stays.</small></span>
      </button>
    {/if}
    {#if left.length}
      <button class="btn way" disabled={busy} onclick={() => free('/downloads/orphans', leftBytes)}>
        <Icon name="trash" size={18} />
        <span><strong>Delete files of titles you left</strong><small>{left.length} {left.length === 1 ? 'title' : 'titles'} no longer in the library, {sized(leftBytes)}.</small></span>
      </button>
    {/if}
    {#if cached}
      <button class="btn way" disabled={busy} onclick={() => free('/cache', cached)}>
        <Icon name="refresh" size={18} />
        <span><strong>Clear cached pages</strong><small>{sized(cached)} kept from reading online. They are not counted in your storage, so this alone does not lift the limit.</small></span>
      </button>
    {/if}
    <button class="btn way" onclick={manage}>
      <Icon name="sliders" size={18} />
      <span><strong>Choose what to delete</strong><small>Downloads in Settings, or a title's own page.</small></span>
    </button>
  </div>

  {#if error}<p class="note" role="alert">{error}</p>{/if}
  {#if freed}<p class="note" role="status">{sized(freed)} freed. Everything opens again within a few minutes, once the room is counted.</p>{/if}

  <div class="foot"><button class="btn" onclick={() => dialog.close()}>Not now</button></div>
</dialog>

<style>
  dialog {
    width: min(460px, calc(100vw - 32px));
    max-height: calc(100dvh - 40px);
    padding: 20px;
    overflow: auto;
    border: 3px solid var(--ink);
    background: var(--surface);
    color: var(--ink);
    box-shadow: var(--sh-8);
  }

  dialog::backdrop {
    background: color-mix(in srgb, var(--ink) 55%, transparent);
  }

  dialog[open] {
    animation: rise 0.18s ease-out;
  }

  @keyframes rise {
    from { transform: translateY(12px); opacity: 0; }
  }

  @media (prefers-reduced-motion: reduce) {
    dialog[open] { animation: none; }
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

  .kicker {
    margin-right: auto;
    font-size: 13px;
    font-weight: 900;
    letter-spacing: 0.12em;
    text-transform: uppercase;
  }

  h2 {
    margin: 14px 0 6px;
    font-size: 30px;
    line-height: 1;
  }

  p {
    margin: 0 0 16px;
    color: var(--muted);
    font-size: 14px;
  }

  .ways {
    display: flex;
    flex-direction: column;
    gap: 10px;
    margin-top: 20px;
  }

  .way {
    justify-content: flex-start;
    width: 100%;
    padding: 10px 14px;
    text-align: left;
  }

  .way span {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .way small {
    font-size: 12px;
    font-weight: 500;
    opacity: 0.8;
  }

  .note {
    margin: 14px 0 0;
    color: var(--ink);
    font-weight: 700;
  }

  .foot {
    display: flex;
    justify-content: flex-end;
    margin-top: 16px;
  }
</style>
