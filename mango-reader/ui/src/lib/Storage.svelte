<script lang="ts">
  // How much of a plan's storage the reader's files take, as a bar. Turns to stripes when
  // nearly full, so it reads without colour.
  import { sized, type Plan } from './plan.svelte';

  let { storage, full = false }: { storage: NonNullable<Plan['storage']>; full?: boolean } = $props();

  const share = $derived(Math.min(1, storage.used / storage.of));
  const near = $derived(full || share >= 0.9);
</script>

<div class="storage">
  <div class="figures">
    <strong class="display">{sized(storage.used)}</strong>
    <span>of {sized(storage.of)}</span>
    <span class="share" class:near>{full ? 'Full' : `${Math.round(share * 100)}%`}</span>
  </div>
  <div class="bar" class:near role="meter" aria-label="Storage used" aria-valuemin={0} aria-valuemax={storage.of} aria-valuenow={Math.min(storage.used, storage.of)} aria-valuetext="{sized(storage.used)} of {sized(storage.of)}">
    <span style:width="{share * 100}%"></span>
  </div>
</div>

<style>
  .storage {
    display: flex;
    flex-direction: column;
    gap: 8px;
    width: 100%;
  }

  .figures {
    display: flex;
    align-items: baseline;
    gap: 8px;
  }

  .figures strong {
    font-size: 26px;
    line-height: 1;
  }

  .figures span {
    color: var(--muted);
    font-size: 13px;
    font-weight: 700;
  }

  .share {
    margin-left: auto;
    padding: 2px 8px;
    border: 2px solid var(--ink);
    color: var(--ink) !important;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  .share.near {
    background: var(--ink);
    color: var(--paper) !important;
  }

  .bar {
    height: 22px;
    border: 3px solid var(--ink);
    background: var(--paper);
    box-shadow: var(--sh-4);
  }

  .bar span {
    display: block;
    height: 100%;
    min-width: 3px;
    border-right: 3px solid var(--ink);
    background: var(--spot);
    transition: width 0.3s ease-out;
  }

  .bar.near span {
    background: repeating-linear-gradient(-45deg, var(--ink) 0 5px, var(--spot) 5px 11px);
  }

  @media (prefers-reduced-motion: reduce) {
    .bar span {
      transition: none;
    }
  }
</style>
