<script lang="ts">
  import { get, message, send, type Category, type Tracker } from '../lib/api';
  import { COVERS, display, MENUS, setDisplay, SIZES } from '../lib/display.svelte';
  import Icon, { type IconName } from '../lib/Icon.svelte';
  import Notice from '../lib/Notice.svelte';
  import { pref, setPref } from '../lib/prefs.svelte';
  import { KINDS, profile, setProfile, type Kind } from '../lib/reader';
  import ReaderPrefs from '../lib/ReaderPrefs.svelte';
  import { route } from '../lib/router.svelte';
  import { session, signOut } from '../lib/session.svelte';
  import { ago } from '../lib/util';
  import { checkVersion, version as about } from '../lib/version.svelte';
  import { activeName, contrast, isHex, palette, PRESETS, saveTheme, sound, SPOTS, theme, type Theme } from '../lib/theme.svelte';

  // One group is on screen at a time. Which one is in the address: #/settings/<id>.
  const TABS: { id: string; label: string; icon: IconName; about: string }[] = [
    { id: 'appearance', label: 'Appearance', icon: 'eye', about: 'Themes, colours and print effects.' },
    { id: 'reader', label: 'Reader', icon: 'book', about: 'How each kind of series is read.' },
    { id: 'library', label: 'Library', icon: 'bookmark', about: 'Categories, and the check for new chapters.' },
    { id: 'downloads', label: 'Downloads', icon: 'download', about: 'Chapters kept on this server.' },
    { id: 'sources', label: 'Sources', icon: 'puzzle', about: 'Where titles come from, and backups in and out.' },
    { id: 'trackers', label: 'Trackers', icon: 'link', about: 'Keep reading progress in step with a tracking site.' },
    { id: 'account', label: 'Account', icon: 'user', about: 'The password, signing out, and who else may use this Mango.' },
    { id: 'about', label: 'About', icon: 'info', about: 'This version, and newer ones.' },
  ];
  const tab = $derived(TABS.find((entry) => entry.id === route.path.split('/')[2]) ?? TABS[0]);

  // On a narrow screen the groups are a strip that scrolls sideways; keep the open one in view.
  let rail = $state<HTMLElement>();
  $effect(() => {
    void tab.id;
    const open = rail?.querySelector<HTMLElement>('[aria-current]');
    if (rail && open) rail.scrollTo({ left: open.offsetLeft - 24 });
  });

  // Body text needs this much contrast between ink and paper to stay readable.
  const ENOUGH = 7;
  // The reader's settings are held once for each kind of series; this is the one on show.
  let kind = $state<Kind>('manga');
  const FOLLOWS: [Theme['follow'], string][] = [
    ['system', 'System'],
    ['light', 'Day'],
    ['dark', 'Night'],
  ];

  let error = $state<string | null>(null);
  let version = $state('');

  const t = $derived(theme.value);
  const d = $derived(display.value);
  const names = [...Object.keys(PRESETS), 'Custom'];

  function change(patch: Partial<Theme>) {
    saveTheme({ ...t, ...patch }).catch((e) => (error = message(e)));
  }

  // ---------- custom colours ----------

  let paper = $state(theme.value.custom.paper);
  let ink = $state(theme.value.custom.ink);
  let agreed = $state(false);
  const ratio = $derived(isHex(paper) && isHex(ink) ? contrast(paper, ink) : 0);
  const weak = $derived(ratio < ENOUGH);

  const slotted = (slot: 'light' | 'dark', name: string): Partial<Theme> => (slot === 'dark' ? { dark: name } : { light: name });

  function useCustom() {
    // Low contrast is the owner's call to make, but never a silent one.
    if (weak && !agreed) {
      agreed = true;
      return;
    }
    agreed = false;
    // They go into whichever of the two themes is showing now.
    const night = t.follow === 'dark' || (t.follow === 'system' && activeName(t) === t.dark && t.dark !== t.light);
    change({ custom: { paper, ink }, ...slotted(night ? 'dark' : 'light', 'Custom') });
  }

  // ---------- import and export ----------

  let pasted = $state('');
  let copied = $state(false);
  const exported = $derived(JSON.stringify(t, null, 2));

  async function copy() {
    await navigator.clipboard.writeText(exported).catch(() => {});
    copied = true;
    setTimeout(() => (copied = false), 2000);
  }

  function importTheme() {
    error = null;
    let raw: unknown;
    try {
      raw = JSON.parse(pasted);
    } catch {
      error = 'That is not a theme: it does not read as JSON.';
      return;
    }
    const next = sound(raw);
    const worst = Math.min(...[next.light, next.dark].map((name) => contrast(palette(next, name).paper, palette(next, name).ink)));
    if (worst < ENOUGH && !agreed) {
      agreed = true;
      error = `This theme's ink on paper has a contrast of ${worst.toFixed(1)} to 1, under the ${ENOUGH} that reads well. Press Import again to use it anyway.`;
      return;
    }
    agreed = false;
    paper = next.custom.paper;
    ink = next.custom.ink;
    pasted = '';
    saveTheme(next).catch((e) => (error = message(e)));
  }

  // ---------- switches kept as settings ----------

  const flip = (key: string) => setPref(key, !pref(key, false)).catch((e) => (error = message(e)));

  function toggleIn(key: string, id: number) {
    const list = pref<number[]>(key, []);
    setPref(key, list.includes(id) ? list.filter((other) => other !== id) : [...list, id]).catch((e) => (error = message(e)));
  }

  // ---------- categories ----------

  let categories = $state<Category[]>([]);
  let fresh = $state('');
  let editing = $state<number | null>(null);
  let renamed = $state('');
  let doomed = $state<number | null>(null);

  async function loadCategories() {
    categories = (await get<{ categories: Category[] }>('/categories')).categories;
  }

  async function attempt(work: () => Promise<unknown>) {
    error = null;
    try {
      await work();
      await loadCategories();
    } catch (e) {
      error = message(e);
    }
  }

  function add(event: SubmitEvent) {
    event.preventDefault();
    attempt(async () => {
      await send('POST', '/categories', { name: fresh });
      fresh = '';
    });
  }

  function rename(event: SubmitEvent, id: number) {
    event.preventDefault();
    attempt(async () => {
      await send('PUT', `/categories/${id}`, { name: renamed });
      editing = null;
    });
  }

  function move(index: number, by: number) {
    const ids = categories.map((c) => c.id);
    const [id] = ids.splice(index, 1);
    ids.splice(index + by, 0, id);
    attempt(() => send('PUT', '/categories', { ids }));
  }

  function remove(id: number) {
    if (doomed !== id) {
      doomed = id;
      return;
    }
    doomed = null;
    attempt(() => send('DELETE', `/categories/${id}`));
  }

  // Only the Library group shows categories, so they are read when it first opens.
  let asked = false;
  $effect(() => {
    if (tab.id !== 'library' || asked) return;
    asked = true;
    attempt(async () => {});
  });

  // ---------- trackers ----------

  const ANILIST_PIN = 'https://anilist.co/api/v2/oauth/pin';
  // Where a tracker that signs in through its own page sends the browser back to.
  const home = location.origin + location.pathname;

  let trackers = $state<Tracker[]>([]);
  let working = $state<number | null>(null);
  // What is typed into each tracker's sign-in form. Sent once, never kept.
  let typed = $state<Record<number, { name: string; secret: string }>>({});

  const clientOf = (t: Tracker) => pref(`trackers.client.${t.id}`, '');
  const setClient = (t: Tracker, value: string) => setPref(`trackers.client.${t.id}`, value.trim()).catch((e) => (error = message(e)));

  async function loadTrackers() {
    const answer = (await get<{ trackers: Tracker[] }>('/trackers')).trackers;
    for (const t of answer) typed[t.id] ??= { name: '', secret: '' };
    trackers = answer;
  }

  async function tracking(t: Tracker, work: () => Promise<unknown>) {
    working = t.id;
    error = null;
    try {
      await work();
      await loadTrackers();
    } catch (e) {
      error = message(e);
    }
    working = null;
  }

  function signIn(event: SubmitEvent, t: Tracker) {
    event.preventDefault();
    const form = typed[t.id];
    tracking(t, async () => {
      await send('POST', `/trackers/${t.id}/login`, t.login === 'token' ? { token: form.secret } : { username: form.name, password: form.secret });
      form.name = form.secret = '';
    });
  }

  // Leaves for the tracker's own page. It sends the browser back to `home` with its answer.
  function leaveFor(t: Tracker) {
    tracking(t, async () => {
      const { url } = await send<{ url: string }>('POST', `/trackers/${t.id}/authorize`, { clientId: clientOf(t), redirect: home });
      location.href = url;
    });
  }

  // For when the tracker's page did not lead back here: the address it led to, pasted in.
  function finish(event: SubmitEvent, t: Tracker) {
    event.preventDefault();
    const form = typed[t.id];
    tracking(t, async () => {
      const answer = new URLSearchParams(form.secret.includes('?') ? form.secret.slice(form.secret.indexOf('?') + 1) : form.secret);
      await send('POST', `/trackers/${t.id}/login`, { code: answer.get('code') ?? '', state: answer.get('state') ?? '' });
      form.secret = '';
    });
  }

  // Read when the group first opens. An answer a tracker's page sent back is handed over first.
  let askedTrackers = false;
  $effect(() => {
    if (tab.id !== 'trackers' || askedTrackers) return;
    askedTrackers = true;
    (async () => {
      const landed = sessionStorage.getItem('tracker.answer');
      sessionStorage.removeItem('tracker.answer');
      try {
        // Only MyAnimeList signs in this way.
        if (landed) await send('POST', '/trackers/1/login', JSON.parse(landed));
      } catch (e) {
        error = message(e);
      }
      await loadTrackers().catch((e) => (error = message(e)));
    })();
  });

  // ---------- account ----------

  let current = $state('');
  let renewed = $state('');
  let accountError = $state<string | null>(null);
  let changed = $state(false);

  async function changePassword(event: SubmitEvent) {
    event.preventDefault();
    accountError = null;
    changed = false;
    try {
      await send('PUT', '/auth/password', { current, password: renewed });
      current = renewed = '';
      changed = true;
    } catch (e) {
      accountError = message(e);
    }
  }

  // ---------- accounts, for an admin ----------

  type Account = { id: number; name: string; admin: boolean; createdAt: number; hasPassword: boolean };

  let users = $state<Account[]>([]);
  let usersError = $state<string | null>(null);
  let newcomer = $state({ name: '', password: '', admin: false });
  /** The account whose new password is being typed, and the one about to be removed. */
  let rekeyed = $state<number | null>(null);
  let rekey = $state('');
  let leaving = $state<number | null>(null);

  async function withUsers(work: () => Promise<unknown>) {
    usersError = null;
    try {
      await work();
      users = (await get<{ users: Account[] }>('/users')).users;
    } catch (e) {
      usersError = message(e);
    }
  }

  function addUser(event: SubmitEvent) {
    event.preventDefault();
    withUsers(async () => {
      await send('POST', '/users', newcomer);
      newcomer = { name: '', password: '', admin: false };
    });
  }

  function setKey(event: SubmitEvent, id: number) {
    event.preventDefault();
    withUsers(async () => {
      await send('PUT', `/users/${id}`, { password: rekey });
      rekeyed = null;
      rekey = '';
    });
  }

  function removeUser(id: number) {
    if (leaving !== id) {
      leaving = id;
      return;
    }
    leaving = null;
    withUsers(() => send('DELETE', `/users/${id}`));
  }

  let askedUsers = false;
  $effect(() => {
    if (tab.id !== 'account' || askedUsers || !session.login || !session.admin) return;
    askedUsers = true;
    withUsers(async () => {});
    if (session.signup) withInvites(async () => {});
  });

  // ---------- invitations, for an admin ----------

  type Invite = { id: number; note: string; createdAt: number; expiresAt: number; usedAt: number | null; usedBy: string | null; code?: string };

  let invites = $state<Invite[]>([]);
  let invitesError = $state<string | null>(null);
  let invitee = $state('');
  /** The invitation just made: the one moment its code can be shown. */
  let made = $state<Invite | null>(null);
  let linkCopied = $state(false);
  // The code stays after the #, which a browser sends to no server.
  const inviteLink = $derived(made?.code ? `${location.origin}${location.pathname}#/join/${made.code}` : '');

  async function withInvites(work: () => Promise<unknown>) {
    invitesError = null;
    try {
      await work();
      invites = (await get<{ invites: Invite[] }>('/invites')).invites;
    } catch (e) {
      invitesError = message(e);
    }
  }

  function addInvite(event: SubmitEvent) {
    event.preventDefault();
    withInvites(async () => {
      made = await send<Invite>('POST', '/invites', { note: invitee });
      invitee = '';
      linkCopied = false;
    });
  }

  function removeInvite(id: number) {
    if (made?.id === id) made = null;
    withInvites(() => send('DELETE', `/invites/${id}`));
  }

  async function copyInvite() {
    try {
      await navigator.clipboard.writeText(inviteLink);
      linkCopied = true;
    } catch {
      invitesError = 'The link could not be copied from here. Select it and copy it by hand.';
    }
  }

  function inviteState(invite: Invite): string {
    if (invite.usedAt) return `Used ${ago(invite.usedAt)}${invite.usedBy ? ` by ${invite.usedBy}` : ''}`;
    if (invite.expiresAt * 1000 < Date.now()) return 'Ran out unused';
    return `Open until ${new Date(invite.expiresAt * 1000).toLocaleDateString()}`;
  }

  // ---------- the solver ----------

  let solver = $state({ url: '', timeoutSecs: 60 });
  let solverError = $state<string | null>(null);
  let solverSaved = $state(false);

  async function saveSolver(event: SubmitEvent) {
    event.preventDefault();
    solverError = null;
    solverSaved = false;
    try {
      solver = await send('PUT', '/solver', solver);
      solverSaved = true;
    } catch (e) {
      solverError = message(e);
    }
  }

  let askedSolver = false;
  $effect(() => {
    if (tab.id !== 'sources' || askedSolver || !session.admin) return;
    askedSolver = true;
    get<typeof solver>('/solver')
      .then((kept) => (solver = kept))
      .catch((e) => (solverError = message(e)));
  });

  // ---------- version ----------

  const v = $derived(about.info);
  let checking = $state(false);
  let versionError = $state<string | null>(null);

  async function check() {
    checking = true;
    versionError = null;
    try {
      await checkVersion();
    } catch (e) {
      versionError = message(e);
    }
    checking = false;
  }

  const day = (iso: string | null) => (iso ? new Date(iso).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' }) : '');

  let localDir = $state('');
  get<{ version: string; localDir: string }>('/health')
    .then((health) => ((version = health.version), (localDir = health.localDir)))
    .catch(() => {});
</script>

{#snippet toggle(on: boolean, label: string, press: () => void)}
  <button class="switch" role="switch" aria-checked={on} aria-label={label} onclick={press}>
    <span class="track"><span class="knob"></span></span>
    <span class="state">{on ? 'On' : 'Off'}</span>
  </button>
{/snippet}

{#snippet swatches(slot: 'light' | 'dark', label: string, hint: string)}
  <div class="set stack" role="group" aria-label={label}>
    <div class="what"><strong>{label}</strong><span>{hint}</span></div>
    <div class="how">
      {#each names as name (name)}
        {@const colours = palette(t, name)}
        <button
          class="btn swatch"
          aria-pressed={t[slot] === name}
          style:background={colours.paper}
          style:color={colours.ink}
          style:border-color={colours.ink}
          onclick={() => change(slotted(slot, name))}
        >
          {#if t[slot] === name}<Icon name="check" size={16} stroke={4} />{/if}{name}
        </button>
      {/each}
    </div>
  </div>
{/snippet}

<div class="page">
  <div class="page-head">
    <h1 class="display">SETTINGS</h1>
  </div>

  <div class="layout">
    <nav class="rail" aria-label="Settings groups" bind:this={rail}>
      {#each TABS as entry (entry.id)}
        <a href="#/settings/{entry.id}" aria-current={entry.id === tab.id ? 'page' : undefined}>
          <Icon name={entry.icon} size={18} />
          <span>{entry.label}</span>
          {#if entry.id === 'about' && v?.updateAvailable}<span class="tag spot">New</span>{/if}
        </a>
      {/each}
    </nav>

    <div class="pane">
      <div class="section-head">
        <h2 class="display">{tab.label.toUpperCase()}</h2>
        <span class="muted">{tab.about}</span>
      </div>

      <Notice {error} />

      {#if tab.id === 'appearance'}
        <section class="card">
          <h3>Theme</h3>
          <div class="set">
            <div class="what"><strong>Show</strong><span>System follows the device between the day and the night theme.</span></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Which theme shows">
                {#each FOLLOWS as [value, label] (value)}
                  <button aria-pressed={t.follow === value} onclick={() => change({ follow: value })}>{label}</button>
                {/each}
              </div>
            </div>
          </div>
          {@render swatches('light', 'Day theme', 'Used when the device is in light mode.')}
          {@render swatches('dark', 'Night theme', 'Used when the device is in dark mode.')}
          <div class="set stack" role="group" aria-label="Spot colour">
            <div class="what"><strong>Spot colour</strong><span>The one accent: main buttons, badges, highlights.</span></div>
            <div class="how">
              {#each Object.entries(SPOTS) as [name, colour] (name)}
                <button class="btn swatch" aria-pressed={t.spot.toUpperCase() === colour} style:background={colour} style:color="#111111" style:border-color="var(--ink)" onclick={() => change({ spot: colour })}>
                  {#if t.spot.toUpperCase() === colour}<Icon name="check" size={16} stroke={4} />{/if}{name}
                </button>
              {/each}
              <label class="btn picker">
                Your own
                <input type="color" value={t.spot} onchange={(e) => change({ spot: e.currentTarget.value })} />
              </label>
            </div>
          </div>
        </section>

        <section class="card">
          <h3>Custom colours</h3>
          <div class="set stack" role="group" aria-label="Custom colours">
            <div class="what"><strong>Paper and ink</strong><span>They go into whichever of the two themes is showing now.</span></div>
            <div class="how">
              <label class="btn picker">Paper <input type="color" bind:value={paper} oninput={() => (agreed = false)} /></label>
              <label class="btn picker">Ink <input type="color" bind:value={ink} oninput={() => (agreed = false)} /></label>
              <span class="sample" style:background={paper} style:color={ink} style:border-color={ink}>Ink on paper, {ratio.toFixed(1)} to 1</span>
              <button class="btn ink" onclick={useCustom}>
                {weak && agreed ? 'Use them anyway' : 'Use these colours'}
              </button>
            </div>
            {#if weak}
              <p class="warn" role="status">
                <strong>Hard to read.</strong> Text wants a contrast of at least {ENOUGH} to 1 between ink and paper; these give {ratio.toFixed(1)}.
                {agreed ? 'Press again to use them anyway.' : ''}
              </p>
            {/if}
          </div>
        </section>

        <section class="card">
          <h3>Print effects</h3>
          <div class="set">
            <div class="what"><strong>Hard shadows</strong><span>The offset ink shadow under covers, panels and buttons.</span></div>
            <div class="how">{@render toggle(t.shadows, 'Hard shadows', () => change({ shadows: !t.shadows }))}</div>
          </div>
          <div class="set">
            <div class="what"><strong>Halftone dots</strong><span>The dotted screen behind covers and empty spaces.</span></div>
            <div class="how">{@render toggle(t.dots, 'Halftone dots', () => change({ dots: !t.dots }))}</div>
          </div>
        </section>

        <section class="card">
          <h3>This device</h3>
          <div class="set">
            <div class="what"><strong>Size</strong><span>Text and controls together. Like the two below, kept on this device alone: a phone and a desk want different answers.</span></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Size of text and controls">
                {#each SIZES as [value, label] (value)}
                  <button aria-pressed={d.size === value} onclick={() => setDisplay({ size: value })}>{label}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="set">
            <div class="what"><strong>Covers</strong><span>Small puts three across a phone's screen, the others two.</span></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Size of covers">
                {#each COVERS as [value, label] (value)}
                  <button aria-pressed={d.covers === value} onclick={() => setDisplay({ covers: value })}>{label}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="set">
            <div class="what"><strong>Menu</strong><span>Along the bottom it is under the thumb. Automatic puts it there on a narrow screen.</span></div>
            <div class="how">
              <div class="seg" role="group" aria-label="Where the menu sits">
                {#each MENUS as [value, label] (value)}
                  <button aria-pressed={d.tabs === value} onclick={() => setDisplay({ tabs: value })}>{label}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="set">
            <div class="what">
              <strong>On a phone's home screen</strong>
              <span>Add Mango to it from the browser's share menu, and it opens as an app of its own, without the address bar.</span>
            </div>
          </div>
        </section>

        <section class="card">
          <h3>Move a theme between devices</h3>
          <div class="set stack">
            <label class="swap">
              <span>This theme</span>
              <textarea readonly rows="6" value={exported}></textarea>
            </label>
            <button class="btn" onclick={copy}>{copied ? 'Copied' : 'Copy'}</button>
          </div>
          <div class="set stack">
            <label class="swap">
              <span>A theme to bring in</span>
              <textarea rows="6" bind:value={pasted} oninput={() => (agreed = false)} placeholder="Paste a theme here"></textarea>
            </label>
            <button class="btn ink" disabled={!pasted.trim()} onclick={importTheme}>Import</button>
          </div>
        </section>
      {:else if tab.id === 'reader'}
        {@const name = KINDS.find((entry) => entry.id === kind)?.name.toLowerCase()}
        <section class="card">
          <h3>Kind of series</h3>
          <div class="set">
            <div class="what">
              <strong>Settings for</strong>
              <span>Mango tells the kinds apart by a title's tags and its source. From inside the reader a title can be told what it is, and keep settings of its own.</span>
            </div>
            <div class="how">
              <div class="seg" role="group" aria-label="Kind of series">
                {#each KINDS as entry (entry.id)}
                  <button aria-pressed={kind === entry.id} onclick={() => (kind = entry.id)}>{entry.name}</button>
                {/each}
              </div>
            </div>
          </div>
          <div class="set">
            <div class="what"><strong>Start over</strong><span>Puts every {name} setting below back to how Mango ships it.</span></div>
            <div class="how"><button class="btn" onclick={() => setProfile(kind, null)}>Reset {name}</button></div>
          </div>
        </section>
        <ReaderPrefs value={profile(kind)} onchange={(patch) => setProfile(kind, patch)} />
      {:else if tab.id === 'library'}
        <section class="card">
          <h3>Categories</h3>
          <ul class="rows">
            {#each categories as c, index (c.id)}
              <li>
                {#if editing === c.id}
                  <form class="main rename" onsubmit={(e) => rename(e, c.id)}>
                    <label class="field"><span class="sr">New name for {c.name}</span><input bind:value={renamed} required maxlength="60" /></label>
                    <button class="btn ink">Save</button>
                    <button class="btn" type="button" onclick={() => (editing = null)}>Cancel</button>
                  </form>
                {:else}
                  <span class="main"><strong>{c.name}</strong></span>
                  <span class="tools">
                    <button class="btn icon" aria-label="Move {c.name} up" disabled={index === 0} onclick={() => move(index, -1)}><Icon name="up" /></button>
                    <button class="btn icon" aria-label="Move {c.name} down" disabled={index === categories.length - 1} onclick={() => move(index, 1)}><Icon name="down" /></button>
                    <button class="btn" onclick={() => ((editing = c.id), (renamed = c.name))}>Rename</button>
                    <button class="btn" onclick={() => remove(c.id)}><Icon name="trash" size={18} />{doomed === c.id ? 'Really remove?' : 'Remove'}</button>
                  </span>
                {/if}
              </li>
            {/each}
          </ul>
          <form class="set" onsubmit={add}>
            <label class="field grow"><span class="sr">Name of a new category</span><Icon name="plus" size={18} stroke={3} /><input bind:value={fresh} placeholder="New category" required maxlength="60" /></label>
            <button class="btn ink">Add category</button>
          </form>
        </section>

        <section class="card">
          <h3>New chapters</h3>
          <div class="set">
            <div class="what"><strong>Check the library</strong><span>How often Mango looks for new chapters by itself.</span></div>
            <div class="how">
              <select class="btn" aria-label="Check the library for new chapters" value={String(pref('updates.hours', 12))} onchange={(e) => setPref('updates.hours', Number(e.currentTarget.value))}>
                <option value="0">Only when asked</option>
                <option value="6">Every 6 hours</option>
                <option value="12">Every 12 hours</option>
                <option value="24">Once a day</option>
                <option value="48">Every 2 days</option>
              </select>
            </div>
          </div>
          <div class="set">
            <div class="what"><strong>Skip completed titles</strong><span>A finished series gets no more chapters.</span></div>
            <div class="how">{@render toggle(pref('updates.skipCompleted', false), 'Skip completed titles', () => flip('updates.skipCompleted'))}</div>
          </div>
          <div class="set">
            <div class="what"><strong>Skip titles with unread chapters</strong><span>Catch up first, then look for more.</span></div>
            <div class="how">{@render toggle(pref('updates.skipUnread', false), 'Skip titles with unread chapters', () => flip('updates.skipUnread'))}</div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Wait for the day a chapter is expected</strong>
              <span>A weekly series is asked around its day, going by the dates of its chapters. A title without dates is asked every time.</span>
            </div>
            <div class="how">{@render toggle(pref('updates.smart', false), 'Wait for the day a chapter is expected', () => flip('updates.smart'))}</div>
          </div>
          {#if categories.length}
            {#each [['updates.categories', 'Only these categories', 'None picked: all of them.'], ['updates.excluded', 'Never these categories', 'Left out even when picked above.']] as [key, label, hint] (key)}
              <div class="set stack" role="group" aria-label={label}>
                <div class="what"><strong>{label}</strong><span>{hint}</span></div>
                <div class="how">
                  {#each categories as c (c.id)}
                    <button class="btn pill" aria-pressed={pref<number[]>(key, []).includes(c.id)} onclick={() => toggleIn(key, c.id)}>{c.name}</button>
                  {/each}
                </div>
              </div>
            {/each}
          {/if}
        </section>
      {:else if tab.id === 'downloads'}
        <section class="card">
          <h3>Files</h3>
          <div class="set">
            <div class="what"><strong>Delete after reading</strong><span>A chapter's file goes once the chapter is read.</span></div>
            <div class="how">{@render toggle(pref('downloads.deleteAfterReading', false), "Delete a chapter's file once it is read", () => flip('downloads.deleteAfterReading'))}</div>
          </div>
          <div class="set">
            <div class="what"><strong>Download queue</strong><span>What is being saved now, and what waits.</span></div>
            <div class="how"><a class="btn" href="#/downloads"><Icon name="download" size={18} />Open the queue</a></div>
          </div>
        </section>
      {:else if tab.id === 'sources'}
        <section class="card">
          <h3>Sources</h3>
          {#if localDir}
            <div class="set stack">
              <div class="what">
                <strong>Local folder</strong>
                <span>One folder per title; inside it a .cbz, a .zip or a folder of images per chapter. They show up under Browse, in “Local”.</span>
              </div>
              <code>{localDir}</code>
            </div>
          {/if}
          <div class="set">
            <div class="what"><strong>Extensions and stores</strong><span>Install, update and remove the extensions that fetch titles.</span></div>
            <div class="how"><a class="btn" href="#/extensions"><Icon name="puzzle" size={18} />Open extensions</a></div>
          </div>
        </section>

        {#if session.admin}
          <section class="card">
            <h3>Cloudflare</h3>
            <form class="set" onsubmit={saveSolver}>
              <div class="what">
                <strong>Solver</strong>
                <span>
                  Some sources sit behind a Cloudflare check that only a browser passes. Mango hands the check to a FlareSolverr, or anything
                  that answers like one, at this address. Empty: no solver.
                </span>
              </div>
              <label class="field grow">
                <span class="sr">The solver's address</span>
                <input type="url" bind:value={solver.url} placeholder="http://localhost:8191" autocomplete="off" />
              </label>
              <label class="field seconds">
                <span class="sr">Seconds a check may take</span>
                <input type="number" bind:value={solver.timeoutSecs} min="5" max="300" title="Seconds a check may take" />
              </label>
              <button class="btn ink">Save</button>
            </form>
            {#if solverError || solverSaved}
              <div class="set stack">
                <Notice error={solverError} />
                {#if solverSaved}<p class="muted" role="status">Saved. The sources use it from their next call.</p>{/if}
              </div>
            {/if}
          </section>
        {/if}

        <section class="card">
          <h3>Reading apps</h3>
          <div class="set stack">
            <div class="what">
              <strong>OPDS catalog</strong>
              <span>
                An app that reads OPDS catalogs can browse this library and fetch its chapters. Give it this
                address{#if session.login}, with your name and password{/if}.
              </span>
            </div>
            <code>{home}opds</code>
          </div>
        </section>

        <section class="card">
          <h3>Backup</h3>
          <div class="set">
            <div class="what"><strong>From Mihon or Suwayomi</strong><span>Bring a library, its categories and reading progress in from a backup file.</span></div>
            <div class="how"><a class="btn" href="#/import"><Icon name="download" size={18} />Import a backup</a></div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Export the library</strong>
              <span>A .tachibk file with the library, its categories, reading progress, history and tracker links. Mihon and Suwayomi read it too.</span>
            </div>
            <div class="how"><a class="btn" href="api/export" download><Icon name="upload" size={18} />Export a backup</a></div>
          </div>
        </section>
      {:else if tab.id === 'trackers'}
        {#each trackers as t (t.id)}
          <section class="card">
            <h3>{t.name}</h3>
            {#if t.signedIn}
              <div class="set">
                <div class="what">
                  <strong>Signed in{t.userName ? ` as ${t.userName}` : ''}</strong>
                  <span>Link a title from its own page. Reading a chapter then moves it along on {t.name}.</span>
                </div>
                <div class="how"><button class="btn" disabled={working === t.id} onclick={() => tracking(t, () => send('DELETE', `/trackers/${t.id}/login`))}>Sign out</button></div>
              </div>
            {:else if t.login === 'password'}
              <form class="set" onsubmit={(e) => signIn(e, t)}>
                <label class="field grow">
                  <span class="sr">Your name on {t.name}</span>
                  <input bind:value={typed[t.id].name} placeholder={t.name === 'Kitsu' ? 'E-mail' : 'User name'} autocomplete="off" required />
                </label>
                <label class="field grow">
                  <span class="sr">Your password on {t.name}</span>
                  <input type="password" bind:value={typed[t.id].secret} placeholder="Password" autocomplete="off" required />
                </label>
                <button class="btn ink" disabled={working === t.id}>Sign in</button>
              </form>
              <div class="set stack"><p class="muted">The password goes to {t.name} once and is not kept. Mango keeps the token it gets back.</p></div>
            {:else if t.login === 'token'}
              <div class="set stack">
                <div class="what">
                  <strong>1. Make an API client</strong>
                  <span>On {t.name}, under Settings, Developer, make a client with the redirect URL below, and put the id it gets here.</span>
                </div>
                <code>{ANILIST_PIN}</code>
                <div class="how">
                  <label class="field">
                    <span class="sr">The client's id</span>
                    <input value={clientOf(t)} inputmode="numeric" placeholder="Client id" autocomplete="off" onchange={(e) => setClient(t, e.currentTarget.value)} />
                  </label>
                  {#if clientOf(t)}
                    <a class="btn" href="https://anilist.co/api/v2/oauth/authorize?client_id={encodeURIComponent(clientOf(t))}&response_type=token" target="_blank" rel="noreferrer">
                      <Icon name="globe" size={18} />Get a token
                    </a>
                  {/if}
                </div>
              </div>
              <form class="set" onsubmit={(e) => signIn(e, t)}>
                <label class="field grow">
                  <span class="sr">The token {t.name} shows</span>
                  <input bind:value={typed[t.id].secret} placeholder="2. Paste the token {t.name} shows" autocomplete="off" required />
                </label>
                <button class="btn ink" disabled={working === t.id}>Sign in</button>
              </form>
            {:else}
              <div class="set stack">
                <div class="what">
                  <strong>1. Register Mango</strong>
                  <span>On {t.name}, under Account Settings, API, make a client of the type “other” with the redirect URL below, and put its client id here.</span>
                </div>
                <code>{home}</code>
                <div class="how">
                  <label class="field">
                    <span class="sr">The client's id</span>
                    <input value={clientOf(t)} placeholder="Client id" autocomplete="off" onchange={(e) => setClient(t, e.currentTarget.value)} />
                  </label>
                  <button class="btn ink" disabled={!clientOf(t) || working === t.id} onclick={() => leaveFor(t)}><Icon name="globe" size={18} />2. Sign in at {t.name}</button>
                </div>
              </div>
              <form class="set" onsubmit={(e) => finish(e, t)}>
                <div class="what"><strong>Did not come back here?</strong><span>Paste the address {t.name} sent the browser to.</span></div>
                <label class="field grow">
                  <span class="sr">The address {t.name} sent the browser to</span>
                  <input bind:value={typed[t.id].secret} placeholder="https://…?code=…" autocomplete="off" required />
                </label>
                <button class="btn" disabled={working === t.id}>Finish</button>
              </form>
            {/if}
          </section>
        {/each}
      {:else if tab.id === 'account'}
        {#if session.login}
          <section class="card">
            <h3>Password</h3>
            <form class="set" onsubmit={changePassword}>
              <input class="sr" type="text" name="username" autocomplete="username" value="mango" tabindex="-1" aria-hidden="true" readonly />
              <label class="field grow">
                <span class="sr">The password in use now</span>
                <input type="password" bind:value={current} placeholder="Current password" autocomplete="current-password" required />
              </label>
              <label class="field grow">
                <span class="sr">The new password</span>
                <input type="password" bind:value={renewed} placeholder="New password, 8 characters or more" autocomplete="new-password" minlength="8" required />
              </label>
              <button class="btn ink">Change password</button>
            </form>
            {#if accountError || changed}
              <div class="set stack">
                <Notice error={accountError} />
                {#if changed}<p class="muted" role="status">The password is changed. Every other browser was signed out.</p>{/if}
              </div>
            {/if}
          </section>

          <section class="card">
            <h3>This browser</h3>
            <div class="set">
              <div class="what">
                <strong>Sign out</strong>
                <span>Signed in as {session.name}. Mango asks for the password again here.</span>
              </div>
              <div class="how"><button class="btn" onclick={() => signOut().catch((e) => (accountError = message(e)))}>Sign out</button></div>
            </div>
          </section>

          {#if session.admin}
            <section class="card">
              <h3>Accounts</h3>
              <div class="set stack">
                <div class="what">
                  <span>
                    Each account has its own stores and extensions, library, reading progress, downloads and settings. An admin is one who
                    may change this list.
                  </span>
                </div>
              </div>
              {#each users as user (user.id)}
                <div class="set">
                  <div class="what">
                    <strong>{user.name}</strong>
                    <span>{user.admin ? 'Admin' : 'Reader'}{user.name === session.name ? ' · you' : ''} · added {ago(user.createdAt)}</span>
                  </div>
                  {#if rekeyed === user.id}
                    <form class="how" onsubmit={(event) => setKey(event, user.id)}>
                      <label class="field grow">
                        <span class="sr">A new password for {user.name}</span>
                        <input type="password" bind:value={rekey} placeholder="New password" autocomplete="new-password" minlength="8" required />
                      </label>
                      <button class="btn ink">Set</button>
                      <button class="btn" type="button" onclick={() => (rekeyed = null)}>Cancel</button>
                    </form>
                  {:else if user.name !== session.name}
                    <div class="how">
                      <button class="btn" onclick={() => ((rekeyed = user.id), (rekey = ''))}>New password</button>
                      {#if user.id !== 1}
                        <button class="btn" onclick={() => withUsers(() => send('PUT', `/users/${user.id}`, { admin: !user.admin }))}>
                          {user.admin ? 'Make reader' : 'Make admin'}
                        </button>
                        <button class="btn" class:ink={leaving === user.id} onclick={() => removeUser(user.id)}>
                          {leaving === user.id ? 'Remove with its library?' : 'Remove'}
                        </button>
                      {/if}
                    </div>
                  {/if}
                </div>
              {/each}
              <form class="set" onsubmit={addUser}>
                <label class="field grow">
                  <span class="sr">The new account's name</span>
                  <input type="text" bind:value={newcomer.name} placeholder="Name" autocomplete="off" maxlength="32" required />
                </label>
                <label class="field grow">
                  <span class="sr">The new account's password</span>
                  <input
                    type="password"
                    bind:value={newcomer.password}
                    placeholder="Password, 8 characters or more"
                    autocomplete="new-password"
                    minlength="8"
                    required
                  />
                </label>
                <label class="check"><input type="checkbox" bind:checked={newcomer.admin} />Admin</label>
                <button class="btn ink">Add account</button>
              </form>
              {#if usersError}
                <div class="set stack"><Notice error={usersError} /></div>
              {/if}
            </section>

            {#if session.signup}
              <section class="card">
                <h3>Invitations</h3>
                <div class="set stack">
                  <div class="what">
                    <span>
                      Whoever holds an invitation makes their own account on the main page, as a reader. Each one opens once and holds a
                      week.
                    </span>
                  </div>
                </div>
                {#if made?.code}
                  <div class="set stack">
                    <div class="what">
                      <strong>Send this link. It is shown this once.</strong>
                      <span>Or the code alone, to type in: {made.code}</span>
                    </div>
                    <div class="how wide">
                      <label class="field grow">
                        <span class="sr">The invitation's link</span>
                        <input type="text" value={inviteLink} readonly onfocus={(event) => event.currentTarget.select()} />
                      </label>
                      <button class="btn ink" onclick={copyInvite}>{linkCopied ? 'Copied' : 'Copy link'}</button>
                    </div>
                  </div>
                {/if}
                {#each invites as invite (invite.id)}
                  <div class="set">
                    <div class="what">
                      <strong>{invite.note || 'No note'}</strong>
                      <span>{inviteState(invite)}</span>
                    </div>
                    <div class="how">
                      <button class="btn" onclick={() => removeInvite(invite.id)}>{invite.usedAt ? 'Forget' : 'Take back'}</button>
                    </div>
                  </div>
                {/each}
                <form class="set" onsubmit={addInvite}>
                  <label class="field grow">
                    <span class="sr">Who the invitation is for</span>
                    <input type="text" bind:value={invitee} placeholder="Who it is for: only admins see this" autocomplete="off" maxlength="80" />
                  </label>
                  <button class="btn ink">Make an invitation</button>
                </form>
                {#if invitesError}
                  <div class="set stack"><Notice error={invitesError} /></div>
                {/if}
              </section>
            {/if}
          {/if}
        {:else}
          <p class="warn"><strong>Login is switched off.</strong> Anyone who can reach this address can use Mango. Take <code>MANGO_LOGIN=off</code> away to ask for a password.</p>
        {/if}
      {:else if v}
        <section class="card">
          <h3>This version</h3>
          <div class="set">
            <div class="what">
              <strong>Mango {v.current}</strong>
              <span>{[v.commit, v.platform, v.docker ? 'Docker' : ''].filter(Boolean).join(' · ')}</span>
            </div>
            <div class="how">
              <button class="btn" disabled={checking || !v.canCheck} onclick={check}><Icon name="refresh" size={18} />{checking ? 'Checking' : 'Check now'}</button>
            </div>
          </div>
          <div class="set stack">
            {#if v.updateAvailable && v.latest}
              <div class="warn" role="status">
                <p><strong>Mango {v.latest.version} is out.</strong></p>
                {#if v.docker}
                  <p>Pull the new image and start the container again: <code>docker compose pull && docker compose up -d</code></p>
                {:else if v.latest.download}
                  <p>Stop Mango, unpack the download over the old files, and start it again. The data folder stays as it is.</p>
                  <p><a class="btn spot" href={v.latest.download.url} rel="noreferrer"><Icon name="download" size={18} />{v.latest.download.name}</a></p>
                {:else}
                  <p>There is no ready-made download for {v.platform}. <a href={v.latest.url} target="_blank" rel="noreferrer">Open the release</a>.</p>
                {/if}
              </div>
            {:else if v.checkedAt && !v.error}
              <p class="muted" role="status">This is the newest release. Checked {ago(v.checkedAt)}.</p>
            {/if}
            {#if v.error}<p class="muted" role="status">{v.error}</p>{/if}
            {#if !v.canCheck}
              <p class="muted">This build was made by hand and does not know where releases are listed. Set <code>MANGO_RELEASES_REPO</code> to <code>owner/name</code> to look for new ones.</p>
            {/if}
            <Notice error={versionError} />
            <p class="muted">{v.checkHours ? `Looked for every ${v.checkHours} hours.` : 'Looked for only when asked.'}</p>
          </div>
        </section>

        {#if v.releases.length}
          <section class="card">
            <h3>Releases and their notes</h3>
            <ul class="rows releases">
              {#each v.releases as release (release.version)}
                <li>
                  <span class="main">
                    <strong>{release.name}</strong>
                    <span>{[day(release.publishedAt), release.prerelease ? 'pre-release' : '', release.version === v.current ? 'this one' : ''].filter(Boolean).join(' · ')}</span>
                    {#if release.notes.trim()}<pre>{release.notes.trim()}</pre>{/if}
                  </span>
                  <a class="btn" href={release.url} target="_blank" rel="noreferrer">Open</a>
                </li>
              {/each}
            </ul>
          </section>
        {/if}
      {:else}
        <p class="muted">{version ? `Mango ${version}` : ''}</p>
      {/if}
    </div>
  </div>
</div>

<style>
  .layout {
    display: grid;
    grid-template-columns: 220px minmax(0, 1fr);
    align-items: start;
    gap: 36px;
  }

  /* ---------- the groups ---------- */

  .rail {
    position: sticky;
    top: 16px;
    display: flex;
    flex-direction: column;
    border: 3px solid var(--ink);
    background: var(--surface);
    box-shadow: var(--sh-5);
  }

  .rail a {
    display: flex;
    flex: none;
    align-items: center;
    gap: 10px;
    min-height: 48px;
    padding: 0 14px;
    border-bottom: 2px solid var(--ink);
    font-size: 13px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    white-space: nowrap;
  }

  .rail a:last-child {
    border-bottom: 0;
  }

  .rail a[aria-current='page'] {
    background: var(--ink);
    color: var(--paper);
  }

  .rail a:focus-visible {
    outline-offset: -5px;
  }

  .rail .tag {
    margin-left: auto;
  }

  .pane {
    display: flex;
    flex-direction: column;
    gap: 24px;
    min-width: 0;
  }

  /* ---------- a card of settings, one per row ---------- */

  .card {
    border: 3px solid var(--ink);
    background: var(--surface);
    box-shadow: var(--sh-5);
  }

  h3 {
    padding: 9px 16px;
    background: var(--ink);
    color: var(--paper);
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.14em;
    text-transform: uppercase;
  }

  .set {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: 10px 24px;
    padding: 14px 16px;
    border-bottom: 2px solid var(--ink);
  }

  .set:last-child {
    border-bottom: 0;
  }

  .set.stack {
    flex-direction: column;
    align-items: flex-start;
  }

  .what {
    display: flex;
    flex: 1 1 240px;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .stack .what {
    flex: none;
  }

  .what strong {
    font-weight: 900;
  }

  .what span {
    color: var(--muted);
    font-size: 13px;
  }

  .how {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
  }

  /* In a stacked row, as wide as the row: for a field that holds a whole address. */
  .how.wide {
    align-self: stretch;
  }

  /* ---------- controls ---------- */

  .switch {
    display: inline-flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0;
    border: 0;
    background: none;
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
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

  .state {
    min-width: 2.4em;
    text-align: left;
  }

  @media (prefers-reduced-motion: reduce) {
    .knob {
      transition: none;
    }
  }

  .seg {
    display: inline-flex;
    border: 2px solid var(--ink);
  }

  .seg button {
    min-height: 40px;
    padding: 0 16px;
    border: 0;
    border-right: 2px solid var(--ink);
    background: var(--surface);
    font-size: 14px;
    font-weight: 700;
  }

  .seg button:last-child {
    border-right: 0;
  }

  .seg button[aria-pressed='true'] {
    background: var(--ink);
    color: var(--paper);
  }

  .seg button:focus-visible {
    outline-offset: -5px;
  }

  .swatch[aria-pressed='true'] {
    box-shadow: var(--sh-4);
  }

  .picker {
    gap: 10px;
  }

  input[type='color'] {
    width: 36px;
    height: 28px;
    padding: 0;
    border: 2px solid var(--ink);
    background: none;
  }

  .sample {
    display: inline-flex;
    align-items: center;
    min-height: 44px;
    padding: 0 14px;
    border: 2px solid;
    font-weight: 700;
  }

  .warn {
    max-width: 640px;
    padding: 12px 14px;
    border: 3px solid var(--ink);
    background: var(--paper);
  }

  .warn p + p {
    margin-top: 8px;
  }

  .swap {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: min(560px, 100%);
    font-size: 12px;
    font-weight: 900;
    letter-spacing: 0.08em;
    text-transform: uppercase;
  }

  textarea {
    width: 100%;
    padding: 10px;
    border: 2px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
    font: 13px/1.4 ui-monospace, Consolas, monospace;
    letter-spacing: 0;
    text-transform: none;
  }

  code {
    overflow-wrap: anywhere;
  }

  /* ---------- lists inside a card ---------- */

  ul {
    margin: 0;
    padding: 0;
    list-style: none;
  }

  li {
    flex-wrap: wrap;
    padding: 6px 16px;
  }

  /* Keeps a name whole: the buttons beside it drop to the next line first. */
  li .main {
    flex-basis: 140px;
  }

  .releases li {
    align-items: flex-start;
  }

  .releases li:last-child {
    border-bottom: 0;
  }

  .rename {
    flex-direction: row;
    flex-wrap: wrap;
    gap: 8px;
  }

  pre {
    margin: 8px 0 0;
    font: 13px/1.5 ui-monospace, Consolas, monospace;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }

  .rename .field,
  .grow {
    flex: 1 1 220px;
    box-shadow: none;
  }

  .seconds {
    flex: 0 0 96px;
    box-shadow: none;
  }

  .check {
    display: flex;
    align-items: center;
    gap: 6px;
    font-weight: 700;
  }

  /* On a narrow screen the groups turn into one strip that scrolls sideways. */
  @media (max-width: 760px) {
    .layout {
      grid-template-columns: minmax(0, 1fr);
      gap: 20px;
    }

    .rail {
      top: var(--safe-top);
      z-index: 2;
      flex-direction: row;
      overflow-x: auto;
      box-shadow: none;
      scrollbar-width: none;
    }

    /* A row of choices takes the whole line, each choice an equal share of it. */
    .how:has(> .seg) {
      flex: 1 1 100%;
    }

    /* The lines between them are the ink showing through, so they hold when the row has to wrap. */
    .seg {
      flex: 1;
      flex-wrap: wrap;
      gap: 2px;
      background: var(--ink);
    }

    .seg button {
      flex: 1 0 auto;
      padding: 0 6px;
      border-right: 0;
    }

    .rail a {
      border-right: 2px solid var(--ink);
      border-bottom: 0;
    }

    .rail a:last-child {
      border-right: 0;
    }
  }
</style>
