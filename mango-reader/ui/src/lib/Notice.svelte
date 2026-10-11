<script lang="ts">
  import type { Snippet } from 'svelte';

  // One place for the three things a screen says instead of its content:
  // still loading, it failed, there is nothing here.
  let {
    loading = false,
    error = null,
    retry,
    children,
  }: { loading?: boolean; error?: string | null; retry?: () => void; children?: Snippet } = $props();
</script>

{#if error}
  <div class="notice problem" role="alert">
    <strong>That did not work.</strong>
    <span>{error}</span>
    {#if retry}<button class="btn" onclick={retry}>Try again</button>{/if}
  </div>
{:else if loading}
  <div class="notice" role="status"><span class="dots" aria-hidden="true"></span>Loading</div>
{:else if children}
  <div class="notice">{@render children()}</div>
{/if}

<style>
  .notice {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px 14px;
    padding: 18px 20px;
    border: 2px dashed var(--ink);
    font-weight: 700;
  }

  .problem {
    border: 3px solid var(--ink);
    background: var(--surface);
    box-shadow: var(--sh-4);
  }

  .problem span {
    flex: 1 1 240px;
    font-weight: 500;
    overflow-wrap: anywhere;
  }

  .dots {
    width: 36px;
    height: 18px;
    background-image: var(--dots);
    background-size: 9px 9px;
    animation: slide 0.8s steps(4) infinite;
  }

  @keyframes slide {
    to {
      background-position: 36px 0;
    }
  }

  @media (prefers-reduced-motion: reduce) {
    .dots {
      animation: none;
    }
  }
</style>
