<script lang="ts">
  import { hash } from './util';

  // Behind every cover sits a printed stand-in: a colour and the title's first
  // character. It shows while the image loads, and stays when there is none.
  const PRINTS = [
    ['#2F6FDE', '#FFFFFF'],
    ['#111111', '#FFC400'],
    ['#E8452C', '#111111'],
    ['#C9CCC6', '#111111'],
    ['#FFC400', '#111111'],
    ['#FFFFFF', '#E8452C'],
  ];

  let { src, title, tilt = false }: { src: string | null; title: string; tilt?: boolean } = $props();

  let failedSrc = $state<string | null>(null);
  const print = $derived(PRINTS[hash(title) % PRINTS.length]);
  const glyph = $derived([...title.trim()][0]?.toUpperCase() ?? '?');
</script>

<span class="cover" class:tilt style:background-color={print[0]} style:color={print[1]}>
  <span class="glyph display" aria-hidden="true">{glyph}</span>
  {#if src && failedSrc !== src}
    <img {src} alt="" loading="lazy" decoding="async" onerror={() => (failedSrc = src)} />
  {/if}
</span>
