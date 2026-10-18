<script lang="ts">
  import { zone, type Prefs } from './reader';

  // Where a tap does what, drawn over the page from the same rule the reader goes by,
  // so it is right for every layout, direction and swap.
  let { p, onclose }: { p: Prefs; onclose: () => void } = $props();

  const N = 12;
  const NAMES = { next: 'Next', prev: 'Back', menu: 'Menu' } as const;
  const cells = $derived(Array.from({ length: N * N }, (_, i) => zone(p, ((i % N) + 0.5) / N, (Math.floor(i / N) + 0.5) / N)));
  /** Each zone's name, at the middle of its cells. */
  const labels = $derived(
    (['prev', 'menu', 'next'] as const).flatMap((hit) => {
      const mine = cells.map((c, i) => (c === hit ? i : -1)).filter((i) => i >= 0);
      if (!mine.length) return [];
      const x = mine.reduce((sum, i) => sum + (i % N) + 0.5, 0) / mine.length / N;
      const y = mine.reduce((sum, i) => sum + Math.floor(i / N) + 0.5, 0) / mine.length / N;
      return [{ hit, x, y }];
    }),
  );
</script>

<button class="zones" aria-label="Tap zones: tap anywhere to close" onclick={onclose}>
  <span class="cells" aria-hidden="true">
    {#each cells as hit, i (i)}<span class={hit}></span>{/each}
  </span>
  {#each labels as label (label.hit)}
    <span class="name" style:left="{label.x * 100}%" style:top="{label.y * 100}%">{NAMES[label.hit]}</span>
  {/each}
  <span class="hint">Tap anywhere to close. Change the zones in the reader’s settings.</span>
</button>

<style>
  .zones {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: block;
    width: 100%;
    padding: 0;
    border: 0;
    background: none;
    color: #111111;
    font: inherit;
    cursor: pointer;
  }

  .cells {
    position: absolute;
    inset: 0;
    display: grid;
    grid-template-columns: repeat(12, 1fr);
    grid-template-rows: repeat(12, 1fr);
    opacity: 0.72;
  }

  .next {
    background: #ffc400;
  }

  .prev {
    background: #5ab0ff;
  }

  .menu {
    background: #e9e9e2;
  }

  .name {
    position: absolute;
    padding: 6px 12px;
    border: 3px solid #111111;
    background: #ffffff;
    font-size: 18px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    transform: translate(-50%, -50%);
  }

  .hint {
    position: absolute;
    right: 12px;
    bottom: max(16px, env(safe-area-inset-bottom));
    left: 12px;
    padding: 10px 12px;
    border: 3px solid #111111;
    background: #ffffff;
    font-weight: 700;
    text-align: center;
  }
</style>
