<script lang="ts">
  import type { Snippet } from 'svelte';
  import Cover from './Cover.svelte';
  import Icon from './Icon.svelte';

  let {
    id,
    title,
    cover,
    inLibrary = false,
    unread = 0,
    onLibrary,
    href,
    meta,
  }: {
    id: number;
    title: string;
    cover: string | null;
    inLibrary?: boolean;
    unread?: number;
    /** Given, the card gets a button that adds the title to the library or takes it out. */
    onLibrary?: () => void;
    /** Where the card leads, when not to the title's own page. */
    href?: string;
    meta?: Snippet;
  } = $props();
</script>

<div class="card">
  <div class="art">
    <a class="face" href={href ?? `#/title/${id}`}>
      <Cover src={cover} {title} />
      <span class="name"><span>{title}</span></span>
    </a>
    {#if unread > 0}
      <span class="unread"><span class="sr">Unread chapters: </span>{unread}</span>
    {/if}
    {#if onLibrary}
      <button
        class="mark"
        class:on={inLibrary}
        aria-pressed={inLibrary}
        aria-label={inLibrary ? `Remove ${title} from the library` : `Add ${title} to the library`}
        onclick={onLibrary}
      >
        <Icon name="bookmark" size={18} fill={inLibrary} />
      </button>
    {/if}
  </div>
  {#if meta}
    <span class="meta">{@render meta()}</span>
  {/if}
</div>

<style>
  .card {
    display: flex;
    flex-direction: column;
    gap: 12px;
    /* Off-screen cards are not laid out or painted, which keeps long grids quick.
       That also clips a card to its own box, so the box is grown by the room the
       shadow and the badge need, and pulled back by the same margins. */
    content-visibility: auto;
    contain-intrinsic-size: auto 300px;
    margin: -14px -12px -8px 0;
    padding: 14px 12px 8px 0;
  }

  .art {
    position: relative;
    container-type: inline-size;
  }

  .face {
    display: block;
  }

  .name {
    position: absolute;
    right: 8px;
    bottom: 8px;
    left: 8px;
    padding: 6px 8px;
    border: 2px solid var(--ink);
    background: var(--surface);
    font-size: 13px;
    font-weight: 900;
    line-height: 1.2;
  }

  /* The clamp sits inside the padding; on the padded box itself a cut-off line would show. */
  .name span {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }

  .unread {
    position: absolute;
    top: -12px;
    right: -10px;
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 38px;
    height: 38px;
    padding: 0 8px;
    border: 3px solid var(--ink);
    border-radius: 19px;
    background: var(--spot);
    color: var(--on-spot);
    font-size: 14px;
    font-weight: 900;
  }

  .mark {
    position: absolute;
    top: 6px;
    left: 6px;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 44px;
    height: 44px;
    border: 2px solid var(--ink);
    background: var(--surface);
  }

  .mark.on {
    background: var(--spot);
    color: var(--on-spot);
  }

  .meta {
    display: flex;
    flex-wrap: wrap;
    justify-content: space-between;
    gap: 0 8px;
    font-size: 12.5px;
    font-weight: 700;
  }

  /* A small cover, three across a phone: the name and the count take less of it. */
  @container (max-width: 125px) {
    .name {
      right: 4px;
      bottom: 4px;
      left: 4px;
      padding: 4px 5px;
      font-size: 11px;
    }

    .unread {
      top: -10px;
      right: -8px;
      min-width: 30px;
      height: 30px;
      padding: 0 6px;
      font-size: 12px;
    }

    .mark {
      top: 0;
      left: 0;
    }
  }
</style>
