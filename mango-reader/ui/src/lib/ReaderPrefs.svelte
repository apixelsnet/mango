<script lang="ts">
  // The reader's settings as rows of controls. Settings shows them for a kind of
  // series, the reader for the chapter that is open: which, the caller decides.
  import { GROUPS, type Field, type Prefs } from './reader';

  let { value, onchange }: { value: Prefs; onchange: (patch: Partial<Prefs>) => void } = $props();

  const set = (key: keyof Prefs, to: unknown) => onchange({ [key]: to } as Partial<Prefs>);
  // Undefined lets a setting go, back to what lies under it.
  const reset = (fields: Field[]) => onchange(Object.fromEntries(fields.map((field) => [field.key, undefined])) as Partial<Prefs>);
</script>

{#each GROUPS as group (group.title)}
  {@const fields = group.fields.filter((field) => !field.when || field.when(value))}
  {#if fields.length}
    <section class="group">
      <h3>
        {group.title}
        <button aria-label="Reset {group.title}" onclick={() => reset(group.fields)}>Reset</button>
      </h3>
      {#each fields as field (field.key)}
        <div class="row">
          <div class="what">
            <strong>{field.label}</strong>
            {#if field.hint}<span>{field.hint}</span>{/if}
          </div>
          {#if field.type === 'choice'}
            <select class="btn" aria-label={field.label} value={value[field.key]} onchange={(e) => set(field.key, e.currentTarget.value)}>
              {#each field.options as [option, label] (option)}<option value={option}>{label}</option>{/each}
            </select>
          {:else if field.type === 'range'}
            <span class="dial">
              <input
                type="range"
                min={field.min}
                max={field.max}
                step={field.step}
                value={value[field.key]}
                aria-label={field.label}
                oninput={(e) => set(field.key, Number(e.currentTarget.value))}
              />
              <output>{value[field.key]}{field.unit}</output>
            </span>
          {:else if field.type === 'switch'}
            <button class="switch" role="switch" aria-checked={!!value[field.key]} aria-label={field.label} onclick={() => set(field.key, !value[field.key])}>
              <span class="track"><span class="knob"></span></span>
            </button>
          {:else}
            <input type="color" aria-label={field.label} value={value[field.key]} oninput={(e) => set(field.key, e.currentTarget.value)} />
          {/if}
        </div>
      {/each}
    </section>
  {/if}
{/each}

<style>
  .group {
    border: 3px solid var(--ink);
    background: var(--surface);
    box-shadow: var(--sh-5);
  }

  h3 {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 0 6px 0 16px;
    background: var(--ink);
    color: var(--paper);
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  h3 button {
    min-height: 36px;
    padding: 0 10px;
    border: 0;
    background: none;
    font-size: 11px;
    font-weight: 900;
    letter-spacing: 0.14em;
    text-transform: uppercase;
    text-decoration: underline;
  }

  h3 button:focus-visible {
    outline-color: var(--paper);
    outline-offset: -4px;
  }

  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 8px 20px;
    padding: 12px 16px;
    border-bottom: 2px solid var(--ink);
  }

  .row:last-child {
    border-bottom: 0;
  }

  .what {
    display: flex;
    flex: 1 1 180px;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .what strong {
    font-weight: 900;
  }

  .what span {
    color: var(--muted);
    font-size: 13px;
  }

  .dial {
    display: flex;
    flex: 1 1 200px;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .dial input {
    flex: 1;
    min-width: 0;
    height: 44px;
    accent-color: var(--ink);
  }

  output {
    min-width: 68px;
    font-weight: 900;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  input[type='color'] {
    width: 52px;
    height: 36px;
    padding: 0;
    border: 2px solid var(--ink);
    background: none;
  }

  .switch {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    padding: 0;
    border: 0;
    background: none;
  }

  .track {
    position: relative;
    width: 52px;
    height: 28px;
    border: 2px solid var(--ink);
    background: var(--paper);
  }

  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 20px;
    height: 20px;
    background: var(--ink);
    transition: transform 0.12s;
  }

  .switch[aria-checked='true'] .track {
    background: var(--spot);
  }

  .switch[aria-checked='true'] .knob {
    background: var(--on-spot);
    transform: translateX(24px);
  }

  @media (prefers-reduced-motion: reduce) {
    .knob {
      transition: none;
    }
  }
</style>
