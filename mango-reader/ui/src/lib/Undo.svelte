<script lang="ts">
  import { undo, word } from './undoing.svelte';

  // Ctrl+Z (⌘Z on a Mac) takes back what the bar offers, outside a text field.
  function key(event: KeyboardEvent) {
    if (!word.undoable || event.key.toLowerCase() !== 'z' || !(event.ctrlKey || event.metaKey) || event.shiftKey || event.altKey) return;
    if (event.target instanceof Element && event.target.closest('input, textarea, select, [contenteditable]')) return;
    event.preventDefault();
    undo();
  }
</script>

<svelte:window onkeydown={key} />

<!-- Always in the page, so a screen reader hears each new word as it comes. -->
<div class="word" class:up={word.text} role="status" aria-live="polite">
  {#if word.text}
    {#key word.turn}
      <span>{word.text}</span>
    {/key}
    {#if word.undoable}<button class="btn ink" onclick={undo}>Undo</button>{/if}
  {/if}
</div>

<style>
  .word {
    position: fixed;
    left: 50%;
    bottom: calc(16px + env(safe-area-inset-bottom));
    z-index: 60;
    display: flex;
    align-items: center;
    gap: 14px;
    width: max-content;
    max-width: min(560px, calc(100vw - 32px));
    padding: 10px 10px 10px 18px;
    border: 3px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
    box-shadow: var(--sh-4);
    font-weight: 700;
    transform: translateX(-50%);
    opacity: 0;
    pointer-events: none;
  }

  .word.up {
    opacity: 1;
    pointer-events: auto;
  }

  .word span {
    flex: 1;
    min-width: 0;
  }

  /* Over the tabs along the bottom of a narrow screen. */
  :global(.shell.tabbed) ~ .word {
    bottom: calc(84px + env(safe-area-inset-bottom));
  }

  @media (prefers-reduced-motion: no-preference) {
    .word.up {
      animation: rise 0.18s ease-out;
    }
  }

  @keyframes rise {
    from {
      transform: translate(-50%, 12px);
      opacity: 0;
    }
  }
</style>
