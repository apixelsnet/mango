<script lang="ts">
  import Notice from './Notice.svelte';

  // Stands in for a screen whose file could not be fetched (App.svelte). Most often Mango was
  // updated while this page was open, and the screen's file has a new name now: the page is
  // reloaded once by itself (lib/lazy.ts), and this says so if that did not help.
  let { ..._ }: Record<string, unknown> = $props();
</script>

<div class="page">
  <Notice error="This screen could not be loaded. Mango may have been updated, or the connection dropped." retry={() => location.reload()} />
</div>
