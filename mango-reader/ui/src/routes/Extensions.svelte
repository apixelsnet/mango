<script lang="ts">
  import { get, message, send, type CatalogItem, type Installed, type Store } from '../lib/api';
  import Icon from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { ago, langName, whenVisible } from '../lib/util';

  const PAGE = 60;

  let stores = $state<Store[]>([]);
  let installed = $state<Installed[]>([]);
  let items = $state<CatalogItem[]>([]);
  let total = $state(0);
  let loading = $state(true);
  let error = $state<string | null>(null);
  let url = $state('');
  let text = $state('');
  let lang = $state('');
  let nsfw = $state(false);
  // What is being worked on, by a key such as "install:pkg", so its button can say so.
  let busy = $state<Record<string, boolean>>({});
  // A destructive button asks once more before it acts.
  let sure = $state('');
  let asked = 0;

  const updates = $derived(installed.filter((e) => e.updateAvailable).length);

  async function attempt(key: string, work: () => Promise<unknown>) {
    busy[key] = true;
    error = null;
    try {
      await work();
    } catch (e) {
      error = message(e);
    }
    busy[key] = false;
  }

  function confirmed(key: string): boolean {
    if (sure === key) {
      sure = '';
      return true;
    }
    sure = key;
    const mine = ++asked;
    setTimeout(() => mine === asked && (sure = ''), 4000);
    return false;
  }

  async function loadCatalog(fresh: boolean) {
    const offset = fresh ? 0 : items.length;
    const content = nsfw ? '&content=safe,mixed,nsfw' : '';
    const page = await get<{ items: CatalogItem[]; total: number }>(
      `/catalog?state=available&limit=${PAGE}&offset=${offset}&q=${encodeURIComponent(text)}&lang=${encodeURIComponent(lang.trim())}${content}`,
    );
    items = fresh ? page.items : [...items, ...page.items];
    total = page.total;
  }

  async function loadAll() {
    const [s, e] = await Promise.all([get<{ stores: Store[] }>('/stores'), get<{ extensions: Installed[] }>('/extensions')]);
    stores = s.stores;
    installed = e.extensions;
    await loadCatalog(true);
  }

  attempt('load', loadAll).then(() => (loading = false));

  let typing: ReturnType<typeof setTimeout>;
  function filter() {
    clearTimeout(typing);
    typing = setTimeout(() => attempt('catalog', () => loadCatalog(true)), 250);
  }

  function addStore(event: SubmitEvent) {
    event.preventDefault();
    attempt('add', async () => {
      await send('POST', '/stores', { url: url.trim() });
      url = '';
      await loadAll();
    });
  }

  const refreshStore = (store: Store) => attempt(`refresh:${store.id}`, async () => { await send('POST', `/stores/${store.id}/refresh`); await loadAll(); });
  const acceptKey = (store: Store) => confirmed(`key:${store.id}`) && attempt(`key:${store.id}`, async () => { await send('POST', `/stores/${store.id}/accept-key`); await loadAll(); });
  const removeStore = (store: Store) => confirmed(`store:${store.id}`) && attempt(`store:${store.id}`, async () => { await send('DELETE', `/stores/${store.id}`); await loadAll(); });
  const install = (pkg: string, store?: number | null) => attempt(`install:${pkg}`, async () => { await send('POST', `/extensions/${pkg}/install${store ? `?store=${store}` : ''}`); await loadAll(); });
  const remove = (pkg: string) => confirmed(`remove:${pkg}`) && attempt(`remove:${pkg}`, async () => { await send('DELETE', `/extensions/${pkg}`); await loadAll(); });
  const updateAll = () => attempt('update-all', async () => { await send('POST', '/extensions/update-all'); await loadAll(); });

  function langs(list: string[]): string {
    const shown = list.slice(0, 3).map(langName).join(', ');
    return list.length > 3 ? `${shown} +${list.length - 3}` : shown;
  }
</script>

<div class="page">
  <div class="page-head">
    <h1 class="display">EXTENSIONS</h1>
    <strong>{installed.length} installed · {stores.length} {stores.length === 1 ? 'store' : 'stores'}</strong>
  </div>

  <Notice {loading} {error} />

  {#if !loading}
    <section>
      <div class="section-head"><h2 class="display">STORES</h2></div>
      <ul class="rows">
        {#each stores as store (store.id)}
          <li class="store">
            <span class="main">
              <strong>{store.name}</strong>
              <span>{store.extensions} extensions{store.checkedAt ? ` · checked ${ago(store.checkedAt)}` : ''}</span>
              {#if store.error && !store.keyChanged}<span>Last check failed: {store.error}</span>{/if}
            </span>
            <span class="tools">
              <button class="btn icon" aria-label="Check {store.name} for new versions" disabled={busy[`refresh:${store.id}`]} onclick={() => refreshStore(store)}>
                <Icon name="refresh" />
              </button>
              <button class="btn" disabled={busy[`store:${store.id}`]} onclick={() => removeStore(store)}>
                <Icon name="trash" size={18} />{sure === `store:${store.id}` ? 'Really remove?' : 'Remove'}
              </button>
            </span>
            {#if store.keyChanged}
              <div class="panel warning" role="alert">
                <strong>This store now signs with a different key.</strong>
                <span>
                  Nothing is installed or updated from it until you decide. Accept the new key only if the store announced the change; otherwise
                  someone may be passing off their own extensions as the store's.
                </span>
                <code>pinned {store.signingKey}</code>
                <code>offered {store.offeredKey}</code>
                <button class="btn" disabled={busy[`key:${store.id}`]} onclick={() => acceptKey(store)}>
                  {sure === `key:${store.id}` ? 'Really trust the new key?' : 'Accept the new key'}
                </button>
              </div>
            {/if}
          </li>
        {/each}
      </ul>
      <form class="add" onsubmit={addStore}>
        <label class="field">
          <span class="sr">Address of a store's repo.json or index.pb</span>
          <Icon name="plus" size={18} stroke={3} />
          <input type="url" bind:value={url} placeholder="https://…/index.pb" required />
        </label>
        <button class="btn ink" disabled={busy.add}>{busy.add ? 'Adding…' : 'Add store'}</button>
      </form>
    </section>

    {#if installed.length}
      <section>
        <div class="section-head">
          <h2 class="display">INSTALLED</h2>
          {#if updates}
            <button class="btn spot" disabled={busy['update-all']} onclick={updateAll}>
              {busy['update-all'] ? 'Updating…' : `Update all (${updates})`}
            </button>
          {/if}
        </div>
        <ul class="rows">
          {#each installed as ext (ext.pkg)}
            <li>
              <a class="main" href="#/extension/{ext.pkg}">
                <strong>{ext.name}</strong>
                <span>
                  {ext.versionName} · {ext.sources.length} {ext.sources.length === 1 ? 'source' : 'sources'}{ext.storeId === null ? ' · its store was removed' : ''}
                </span>
              </a>
              <span class="tools">
                {#if ext.contentWarning >= 3}<span class="tag">18+</span>{/if}
                <a class="btn icon" href="#/extension/{ext.pkg}" aria-label="Settings for {ext.name}" title="Settings"><Icon name="sliders" size={18} /></a>
                {#if ext.updateAvailable}
                  <button class="btn spot" disabled={busy[`install:${ext.pkg}`]} onclick={() => install(ext.pkg, ext.storeId)}>
                    {busy[`install:${ext.pkg}`] ? 'Updating…' : `Update to ${ext.availableVersion}`}
                  </button>
                {/if}
                <button class="btn" disabled={busy[`remove:${ext.pkg}`]} onclick={() => remove(ext.pkg)}>
                  {sure === `remove:${ext.pkg}` ? 'Really remove?' : 'Remove'}
                </button>
              </span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if stores.length}
      <section>
        <div class="section-head">
          <h2 class="display">AVAILABLE</h2>
          <div class="tools">
            <label class="field find">
              <span class="sr">Find an extension</span>
              <Icon name="search" size={18} stroke={3} />
              <input type="search" bind:value={text} oninput={filter} placeholder="Find an extension" />
            </label>
            <label class="field code">
              <span class="sr">Language codes, comma separated</span>
              <input bind:value={lang} oninput={filter} placeholder="en, ja" />
            </label>
            <button class="btn" aria-pressed={nsfw} onclick={() => ((nsfw = !nsfw), filter())}>Show 18+</button>
          </div>
        </div>
        <p class="muted count">{total} {total === 1 ? 'extension' : 'extensions'}{nsfw ? '' : ', 18+ hidden'}</p>
        <ul class="rows">
          {#each items as item (`${item.storeId}:${item.pkg}`)}
            <li>
              <span class="main">
                <strong>{item.name}</strong>
                <span>{item.versionName} · {langs(item.languages)} · {item.sourceCount} {item.sourceCount === 1 ? 'source' : 'sources'}</span>
              </span>
              <span class="tools">
                {#if item.contentWarning >= 3}<span class="tag">18+</span>{/if}
                <button class="btn" disabled={!item.installable || busy[`install:${item.pkg}`]} onclick={() => install(item.pkg, item.storeId)}>
                  {!item.installable ? 'APK only' : busy[`install:${item.pkg}`] ? 'Installing…' : 'Install'}
                </button>
              </span>
            </li>
          {/each}
        </ul>
        {#if items.length < total}
          <div class="more" use:whenVisible={() => !busy.catalog && attempt('catalog', () => loadCatalog(false))}>
            <button class="btn" disabled={busy.catalog} onclick={() => attempt('catalog', () => loadCatalog(false))}>Load more</button>
          </div>
        {/if}
      </section>
    {:else}
      <Notice>Add a store above to see the extensions it offers.</Notice>
    {/if}
  {/if}
</div>

<style>
  section {
    display: flex;
    flex-direction: column;
    gap: 12px;
  }

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .store {
    flex-wrap: wrap;
  }

  .warning {
    display: flex;
    flex: 1 1 100%;
    flex-direction: column;
    align-items: flex-start;
    gap: 8px;
    margin: 8px 8px 12px 0;
    padding: 16px;
  }

  code {
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  .add {
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    padding-top: 8px;
  }

  .add .field {
    flex: 1 1 280px;
  }

  .find {
    flex: 1 1 220px;
  }

  .code {
    flex: 0 1 110px;
  }

  .count {
    font-size: 13px;
    font-weight: 700;
  }

  .more {
    display: flex;
    justify-content: center;
    padding-top: 12px;
  }
</style>
