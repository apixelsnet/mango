<!--
  Something Mango shows once: a password or a list of codes. Hidden until asked for, so
  that it does not sit on a screen someone else can see; it can be copied, and with a
  file name given, saved as a text file.
-->
<script lang="ts">
  import Icon from './Icon.svelte';
  import { copyText } from './util';

  let { values, label, file = '', heading = '' }: { values: string[]; label: string; file?: string; heading?: string } = $props();

  let shown = $state(false);
  let copied = $state<boolean | null>(null);
  let timer: ReturnType<typeof setTimeout> | undefined;

  async function copy() {
    copied = await copyText(values.join('\n'));
    clearTimeout(timer);
    timer = setTimeout(() => (copied = null), 2000);
  }

  function save() {
    const text = (heading ? `${heading}\n\n` : '') + values.join('\n') + '\n';
    const link = document.createElement('a');
    link.href = URL.createObjectURL(new Blob([text], { type: 'text/plain' }));
    link.download = file;
    link.click();
    setTimeout(() => URL.revokeObjectURL(link.href), 1000);
  }
</script>

<div class="secret">
  <ul class:many={values.length > 1} aria-label={label}>
    {#each values as value, index (index)}
      <li><code class:hidden={!shown}>{shown ? value : '•'.repeat(value.length)}</code></li>
    {/each}
  </ul>
  <div class="tools">
    <button class="btn" type="button" aria-pressed={shown} onclick={() => (shown = !shown)}>
      <Icon name={shown ? 'eyeoff' : 'eye'} size={18} />{shown ? 'Hide' : 'Show'}
    </button>
    <button class="btn" type="button" onclick={copy}><Icon name="copy" size={18} />{copied ? 'Copied' : copied === false ? 'Could not copy' : 'Copy'}</button>
    {#if file}
      <button class="btn" type="button" onclick={save}><Icon name="download" size={18} />Download</button>
    {/if}
  </div>
</div>

<style>
  .secret {
    display: flex;
    flex-direction: column;
    gap: 10px;
    align-self: stretch;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  /* A row that wraps: a grid of filled columns takes the height of one column in a stacked setting. */
  .many {
    display: flex;
    flex-wrap: wrap;
    gap: 6px 24px;
  }

  li {
    padding: 0;
  }

  code {
    font: 15px/1.5 ui-monospace, Consolas, monospace;
    letter-spacing: 0.06em;
    user-select: all;
  }

  .hidden {
    user-select: none;
  }

  .tools {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
</style>
