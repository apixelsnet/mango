<script lang="ts">
  // A mark beside what a hosted reader's plan leaves out. It leads to the account page,
  // where a plan that has it can be picked.
  import Icon from './Icon.svelte';
  import { session } from './session.svelte';

  let { label = 'Not in your plan' }: { label?: string } = $props();
</script>

<a class="locked" href={session.account} title="Your plan leaves this out. The plans are on your account page.">
  <Icon name="lock" size={14} stroke={3} />{label}
</a>

<style>
  .locked {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    min-height: 28px;
    padding: 3px 9px;
    border: 2px solid var(--ink);
    background: var(--spot);
    color: var(--on-spot);
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.1em;
    text-transform: uppercase;
    text-decoration: none;
    white-space: nowrap;
  }

  .locked:hover {
    background: var(--ink);
    color: var(--paper);
  }
</style>
