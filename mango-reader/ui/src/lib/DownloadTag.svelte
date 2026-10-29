<script lang="ts">
  import type { Queued } from './api';
  import Icon from './Icon.svelte';
  import type { Waits } from './queue';

  /** Where a chapter's download stands: saved, waiting, being fetched, or given up on. Nothing when none of these. */
  let { saved = false, entry, waits = '' }: { saved?: boolean; entry?: Queued; waits?: Waits } = $props();

  function says(entry: Queued): string {
    if (entry.state === 'downloading') return entry.total ? `${Math.min(100, Math.round((entry.done / entry.total) * 100))}%` : 'Downloading';
    if (entry.state === 'failed') return 'Failed';
    if (entry.error) return 'Retrying';
    return waits === 'held' ? 'On hold' : waits === 'paused' ? 'Paused' : 'Queued';
  }
</script>

{#if saved}
  <span class="tag ink fetch" title="Saved on the server: it opens without asking the source"><Icon name="check" size={12} stroke={4} />Saved</span>
{:else if entry}
  <span
    class="tag fetch"
    class:spot={entry.state === 'downloading'}
    class:failed={entry.state === 'failed'}
    title={entry.error ?? (entry.state === 'downloading' ? 'Downloading' : 'In the download queue')}
  >
    <Icon name={entry.state === 'downloading' ? 'download' : entry.state === 'failed' ? 'close' : 'clock'} size={12} stroke={4} />
    {#if entry.state === 'downloading'}<span class="sr">Downloading,</span>{/if}{says(entry)}
  </span>
{/if}

<style>
  .fetch {
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 4px;
  }

  .failed {
    border-style: dashed;
  }
</style>
