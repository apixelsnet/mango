<script lang="ts">
  import { get, message, send, type Backup, type Category, type Orphan, type ReadFiles, type Tracker } from '../lib/api';
  import { COVERS, detectDevice, DEVICES, display, MENUS, PAGES, setDisplay, SIZES, thisDevice, type Device } from '../lib/display.svelte';
  import Icon, { type IconName } from '../lib/Icon.svelte';
  import Locked from '../lib/Locked.svelte';
  import Notice from '../lib/Notice.svelte';
  import Secret from '../lib/Secret.svelte';
  import { lacks, loadPlan, plan, tooOften } from '../lib/plan.svelte';
  import { pref, setPref } from '../lib/prefs.svelte';
  import * as push from '../lib/push';
  import { ACTIONS, byDevice, FIXED, keyName, KINDS, profile, setProfile, type Action, type Kind } from '../lib/reader';
  import ReaderPrefs from '../lib/ReaderPrefs.svelte';
  import { route } from '../lib/router.svelte';
  import { session, signOut } from '../lib/session.svelte';
  import Storage from '../lib/Storage.svelte';
  import { againTour } from '../lib/guide.svelte';
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
    { id: 'about', label: 'About', icon: 'info', about: 'The tour, this version, and newer ones.' },
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
  // Which kind of device the reader settings below are for: this one, unless another is picked.
  let picked = $state<Device | null>(null);
  const here = $derived(thisDevice());
  const forDevice = $derived(picked ?? here);
  const deviceName = (device: Device) => DEVICES.find(([each]) => each === device)?.[1] ?? device;
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

  // ---------- two-factor authentication and app passwords ----------

  type TwoStep = { on: boolean; codesLeft: number };
  type AppPassword = { id: number; name: string; createdAt: number; usedAt: number | null; password?: string };

  let twoStep = $state<TwoStep | null>(null);
  /** The authenticator being set up: its key, its link, and the link as a picture to scan. */
  let enrolment = $state<{ secret: string; uri: string; picture: string } | null>(null);
  let twoStepCode = $state('');
  let twoStepPassword = $state('');
  /** Recovery codes, shown once, just after they were made. */
  let recovery = $state<string[]>([]);
  let twoStepError = $state<string | null>(null);

  let apps = $state<AppPassword[]>([]);
  let appName = $state('');
  /** The app password just made, with the password, shown this once. */
  let madeApp = $state<AppPassword | null>(null);
  let appsError = $state<string | null>(null);

  async function withTwoStep(work: () => Promise<unknown>) {
    twoStepError = null;
    try {
      await work();
      twoStep = await get<TwoStep>('/auth/two-step');
    } catch (e) {
      twoStepError = message(e);
    }
  }

  function startTwoStep() {
    withTwoStep(async () => {
      const offer = await send<{ secret: string; uri: string }>('POST', '/auth/two-step');
      // Only this step needs the picture maker, so it loads only now.
      const { default: qrcode } = await import('qrcode-generator');
      const qr = qrcode(0, 'M');
      qr.addData(offer.uri);
      qr.make();
      enrolment = { ...offer, picture: qr.createSvgTag({ cellSize: 4, margin: 2, scalable: true }) };
      twoStepCode = '';
      recovery = [];
    });
  }

  function confirmTwoStep(event: SubmitEvent) {
    event.preventDefault();
    withTwoStep(async () => {
      recovery = (await send<{ codes: string[] }>('PUT', '/auth/two-step', { code: twoStepCode })).codes;
      enrolment = null;
    });
  }

  function newRecovery(event: SubmitEvent) {
    event.preventDefault();
    withTwoStep(async () => {
      recovery = (await send<{ codes: string[] }>('POST', '/auth/two-step/codes', { password: twoStepPassword })).codes;
      twoStepPassword = '';
    });
  }

  function leaveTwoStep() {
    if (!twoStepPassword) {
      twoStepError = 'Type your password first.';
      return;
    }
    withTwoStep(async () => {
      await send('DELETE', '/auth/two-step', { password: twoStepPassword });
      twoStepPassword = '';
      recovery = [];
    });
  }

  async function withApps(work: () => Promise<unknown>) {
    appsError = null;
    try {
      await work();
      apps = (await get<{ apps: AppPassword[] }>('/auth/apps')).apps;
    } catch (e) {
      appsError = message(e);
    }
  }

  function addApp(event: SubmitEvent) {
    event.preventDefault();
    withApps(async () => {
      madeApp = await send<AppPassword>('POST', '/auth/apps', { name: appName });
      appName = '';
    });
  }

  function removeApp(id: number) {
    withApps(async () => {
      await send('DELETE', `/auth/apps/${id}`);
      if (madeApp?.id === id) madeApp = null;
    });
  }

  let askedSignIn = false;
  $effect(() => {
    if (tab.id !== 'account' || askedSignIn || !session.login || !session.signedIn) return;
    askedSignIn = true;
    // A hosted Mango's own sign-in page asks for no code yet, so two-factor authentication stays off there.
    if (!session.account) withTwoStep(async () => {});
    withApps(async () => {});
  });

  // ---------- accounts, for an admin ----------

  type Account = { id: number; name: string; admin: boolean; createdAt: number; hasPassword: boolean; twoStep: boolean };

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

  // What a hosted reader's plan leaves out, said in words on the account group.
  const leftOut = $derived(
    plan.is
      ? [
          !plan.is.trackers && 'Trackers',
          !plan.is.autoDownload && 'Downloading new chapters by themselves',
          !plan.is.opds && 'Reading apps over OPDS',
          tooOften(6) && `Checking the library by itself more often than every ${plan.is.updatesEvery} hours`,
        ].filter((what): what is string => typeof what === 'string')
      : [],
  );

  // A hosted reader's plan and storage, fresh each time the account group opens.
  $effect(() => {
    if (tab.id === 'account' && session.account) loadPlan(true);
  });

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

  // What "Export my files" would hold, asked for when the Downloads group opens.
  type FilesSize = { titles: number; chapters: number; bytes: number };
  let files = $state<FilesSize | null>(null);
  $effect(() => {
    if (tab.id !== 'downloads') return;
    get<FilesSize>('/export/files/size')
      .then((size) => (files = size))
      .catch(() => (files = null));
  });
  const counted = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'}`;
  const sized = (bytes: number) => (bytes >= 1 << 30 ? `${(bytes / (1 << 30)).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / (1 << 20)))} MB`);

  // ---------- titles left behind with files ----------

  let orphans = $state<Orphan[]>([]);
  let clearing = $state(false);
  $effect(() => {
    if (tab.id !== 'downloads') return;
    get<Orphan[]>('/downloads/orphans')
      .then((found) => (orphans = found))
      .catch(() => (orphans = []));
  });

  async function clearOrphans() {
    if (!clearing) {
      clearing = true;
      return;
    }
    clearing = false;
    error = null;
    try {
      await send('DELETE', '/downloads/orphans');
      orphans = [];
    } catch (e) {
      error = message(e);
    }
  }

  // How often the library may be checked by itself, in hours; 0 is only when asked.
  const CHECKS = [
    [0, 'Only when asked'],
    [6, 'Every 6 hours'],
    [12, 'Every 12 hours'],
    [24, 'Once a day'],
    [48, 'Every 2 days'],
  ] as const;

  // ---------- the page cache ----------

  let cachedBytes = $state<number | null>(null);
  $effect(() => {
    if (tab.id !== 'downloads') return;
    get<{ bytes: number }>('/cache')
      .then((found) => (cachedBytes = found.bytes))
      .catch(() => (cachedBytes = null));
  });

  async function clearCache() {
    error = null;
    try {
      await send('DELETE', '/cache');
      cachedBytes = 0;
    } catch (e) {
      error = message(e);
    }
  }

  // ---------- files of chapters already read ----------

  let readFiles = $state<ReadFiles | null>(null);
  let clearingRead = $state(false);
  $effect(() => {
    if (tab.id !== 'downloads') return;
    get<ReadFiles>('/downloads/read')
      .then((found) => (readFiles = found))
      .catch(() => (readFiles = null));
  });

  async function clearRead() {
    if (!clearingRead) {
      clearingRead = true;
      return;
    }
    clearingRead = false;
    error = null;
    try {
      await send('DELETE', '/downloads/read');
      readFiles = { chapters: 0, bytes: 0 };
      loadPlan(true);
    } catch (e) {
      error = message(e);
    }
  }

  // ---------- notifications ----------

  let notifyNote = $state('');

  async function testNotify() {
    notifyNote = 'Sending…';
    try {
      await send('POST', '/notify/test');
      notifyNote = 'Sent. If nothing arrived, check the address.';
    } catch (e) {
      notifyNote = message(e);
    }
  }

  // ---------- notifications on this device ----------

  const pushUnable = push.unable();
  let pushOn = $state(false);
  let pushDevices = $state(0);
  let pushBusy = $state(false);
  let pushNote = $state('');

  async function loadPush() {
    try {
      ({ on: pushOn, devices: pushDevices } = await push.state());
    } catch (e) {
      pushNote = message(e);
    }
  }

  $effect(() => {
    if (tab.id === 'library' && !pushUnable) loadPush();
  });

  async function flipPush() {
    if (pushBusy) return;
    pushBusy = true;
    pushNote = '';
    try {
      await (pushOn ? push.turnOff() : push.turnOn());
      await loadPush();
    } catch (e) {
      pushNote = message(e);
    } finally {
      pushBusy = false;
    }
  }

  async function testPush() {
    pushNote = 'Sending…';
    try {
      const sent = await push.test();
      pushNote = sent ? `Sent to ${sent} ${sent === 1 ? 'device' : 'devices'}.` : 'No device took it. Turn notifications off and on again here.';
      await loadPush();
    } catch (e) {
      pushNote = message(e);
    }
  }

  // ---------- backups made by themselves ----------

  let backups = $state<Backup[]>([]);
  let backingUp = $state(false);

  async function loadBackups() {
    backups = (await get<{ backups: Backup[] }>('/backups')).backups;
  }

  $effect(() => {
    if (tab.id === 'sources') loadBackups().catch(() => {});
  });

  async function backUpNow() {
    backingUp = true;
    error = null;
    try {
      await send('POST', '/backups');
      await loadBackups();
    } catch (e) {
      error = message(e);
    }
    backingUp = false;
  }

  function removeBackup(name: string) {
    attempt(async () => {
      await send('DELETE', `/backups/${name}`);
      await loadBackups();
    });
  }

  // ---------- keys of one's own ----------

  // The action waiting for its new key.
  let listening = $state<Action | null>(null);
  const ownKeys = $derived(pref<Partial<Record<Action, string[]>>>('reader.keys', {}));
  const keysOf = (action: (typeof ACTIONS)[number]) => ownKeys[action.id] ?? action.keys;
  const shownKey = (key: string) => (key === ' ' ? 'Space' : key.length === 1 ? key.toUpperCase() : key);

  /** The next key pressed goes to the action listened for; another action that had it loses it. */
  function takeKey(event: KeyboardEvent) {
    if (!listening) return;
    event.preventDefault();
    if (event.key === 'Escape') {
      listening = null;
      return;
    }
    if (FIXED.includes(event.key) || ['Shift', 'Control', 'Alt', 'Meta'].includes(event.key)) return;
    const key = keyName(event.key);
    const next: Partial<Record<Action, string[]>> = {};
    for (const action of ACTIONS) {
      const kept = keysOf(action).filter((k) => keyName(k) !== key);
      next[action.id] = action.id === listening ? [key] : kept;
    }
    listening = null;
    setPref('reader.keys', next).catch((e) => (error = message(e)));
  }
</script>

<svelte:window onkeydown={takeKey} />

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
            <div class="what"><strong>Still screen</strong><span>Nothing moves or fades, and there are no shadows: for an e-ink reader, which shows every frame of a movement.</span></div>
            <div class="how">{@render toggle(d.still, 'Still screen', () => setDisplay({ still: !d.still }))}</div>
          </div>
          <div class="set">
            <div class="what"><strong>Library in pages</strong><span>A page of titles at a time, turned with buttons, instead of one long list.</span></div>
            <div class="how">
              <select class="btn" aria-label="Library in pages" value={String(d.pages)} onchange={(e) => setDisplay({ pages: Number(e.currentTarget.value) })}>
                {#each PAGES as [value, label] (value)}<option value={String(value)}>{label}</option>{/each}
              </select>
            </div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Incognito</strong>
              <span>Reading on this device leaves no trace: no place kept, no history, nothing sent to trackers, nothing downloaded ahead.</span>
            </div>
            <div class="how">{@render toggle(d.incognito, 'Incognito', () => setDisplay({ incognito: !d.incognito }))}</div>
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
        {@const apart = byDevice()}
        <section class="card">
          <h3>Devices</h3>
          <div class="set">
            <div class="what">
              <strong>Each device its own</strong>
              <span>Page size, two pages, strip width, tap zones, scrolling and pages loaded ahead are kept apart for phones, tablets and computers: fit height on a desk, fit width on a phone. Every phone shares one set, so a new phone reads like the last. Off, every device reads the same.</span>
            </div>
            <div class="how">{@render toggle(apart, 'Each device its own', () => setPref('reader.byDevice', !apart).catch((e) => (error = message(e))))}</div>
          </div>
          <div class="set">
            <div class="what">
              <strong>This device is</strong>
              <span>Told by its screen: a small touch screen is a phone, a large one a tablet, one with a mouse a computer. Kept on this device alone.</span>
            </div>
            <div class="how">
              <select class="btn" aria-label="This device is" value={display.value.device} onchange={(e) => ((picked = null), setDisplay({ device: e.currentTarget.value as 'auto' | Device }))}>
                <option value="auto">Automatic ({deviceName(detectDevice())})</option>
                {#each DEVICES as [value, label] (value)}<option {value}>{label}</option>{/each}
              </select>
            </div>
          </div>
          {#if apart}
            <div class="set">
              <div class="what">
                <strong>Settings for</strong>
                <span>The marked settings below are for this kind of device. Pick another to set it up from here.</span>
              </div>
              <div class="how">
                <div class="seg" role="group" aria-label="Kind of device">
                  {#each DEVICES as [value, label] (value)}
                    <button aria-pressed={forDevice === value} onclick={() => (picked = value)}>{label}{value === here ? ' (this one)' : ''}</button>
                  {/each}
                </div>
              </div>
            </div>
          {/if}
        </section>
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
        <ReaderPrefs value={profile(kind, forDevice)} onchange={(patch) => setProfile(kind, patch, forDevice)} device={apart ? deviceName(forDevice) : undefined} />

        <section class="card">
          <h3>While reading</h3>
          <div class="set">
            <div class="what"><strong>Download ahead</strong><span>Opening a chapter of a library title queues the next few for download, so they read with no wait or no connection.</span></div>
            <div class="how">
              <select class="btn" aria-label="Chapters downloaded ahead" value={String(pref('reader.downloadAhead', 0))} onchange={(e) => setPref('reader.downloadAhead', Number(e.currentTarget.value))}>
                <option value="0">None</option>
                {#each [1, 2, 3, 5, 10] as n (n)}<option value={String(n)}>The next {n}</option>{/each}
              </select>
            </div>
          </div>
          <div class="set">
            <div class="what"><strong>Mark other uploads read</strong><span>Reading a chapter to its end marks its uploads by other groups, or from other sources of the title, read too.</span></div>
            <div class="how">{@render toggle(pref('chapters.markDuplicates', false), 'Mark other uploads read', () => flip('chapters.markDuplicates'))}</div>
          </div>
        </section>

        <section class="card">
          <h3>Keys</h3>
          <p class="muted">What each key does in the reader. Arrows, Space, Home, End and Escape keep their own work.</p>
          <ul class="rows">
            {#each ACTIONS as action (action.id)}
              <li>
                <span class="main"><strong>{action.label}</strong><span>{keysOf(action).map(shownKey).join(', ') || 'No key'}</span></span>
                <button class="btn" aria-pressed={listening === action.id} onclick={() => (listening = listening === action.id ? null : action.id)}>
                  {listening === action.id ? 'Press a key…' : 'Change'}
                </button>
              </li>
            {/each}
          </ul>
          <div class="set">
            <div class="what"><strong>Start over</strong><span>Every key back to what Mango ships.</span></div>
            <div class="how"><button class="btn" onclick={() => setPref('reader.keys', null)}>Reset keys</button></div>
          </div>
        </section>
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
                    <select
                      class="btn"
                      aria-label="Where {c.name} sits"
                      value={String(c.parent ?? 0)}
                      onchange={(e) => attempt(() => send('PUT', `/categories/${c.id}`, { parent: Number(e.currentTarget.value) }))}
                    >
                      <option value="0">At the top</option>
                      {#each categories.filter((other) => other.id !== c.id) as other (other.id)}<option value={String(other.id)}>Inside {other.name}</option>{/each}
                    </select>
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
              {#if plan.is?.updatesEvery && tooOften(6)}<Locked label="Every {plan.is.updatesEvery} h at most" />{/if}
              <select class="btn" aria-label="Check the library for new chapters" value={String(pref('updates.hours', 12))} onchange={(e) => setPref('updates.hours', Number(e.currentTarget.value))}>
                {#each CHECKS as [hours, label] (hours)}
                  <option value={String(hours)} disabled={tooOften(hours)}>{label}{tooOften(hours) ? ' (not in your plan)' : ''}</option>
                {/each}
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
            <div class="what"><strong>Skip titles not started</strong><span>A title none of whose chapters was opened yet waits until one is.</span></div>
            <div class="how">{@render toggle(pref('updates.skipUnstarted', false), 'Skip titles not started', () => flip('updates.skipUnstarted'))}</div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Wait for the day a chapter is expected</strong>
              <span>A weekly series is asked around its day, going by the dates of its chapters. A title without dates is asked every time.</span>
            </div>
            <div class="how">{@render toggle(pref('updates.smart', false), 'Wait for the day a chapter is expected', () => flip('updates.smart'))}</div>
          </div>
          <div class="set stack">
            <div class="what">
              <strong>Tell me of new chapters</strong>
              <span>
                After each check that finds new chapters, Mango sends word to this address: an ntfy topic, a Discord channel's webhook, or any address that takes a JSON
                post. It has to be on the internet and HTTPS, unless whoever runs this Mango allowed otherwise.
              </span>
            </div>
            <div class="how">
              <label class="field grow">
                <span class="sr">Address for notifications</span>
                <input type="url" value={pref('notify.url', '')} placeholder="https://ntfy.sh/your-topic" autocomplete="off" onchange={(e) => setPref('notify.url', e.currentTarget.value.trim() || null)} />
              </label>
              <select class="btn" aria-label="Kind of address" value={pref('notify.kind', 'ntfy')} onchange={(e) => setPref('notify.kind', e.currentTarget.value)}>
                <option value="ntfy">ntfy</option>
                <option value="discord">Discord</option>
                <option value="webhook">Webhook (JSON)</option>
              </select>
              <button class="btn" disabled={!pref('notify.url', '')} onclick={testNotify}>Send a test</button>
            </div>
            {#if notifyNote}<p class="muted" role="status">{notifyNote}</p>{/if}
          </div>
          <div class="set stack">
            <div class="what">
              <strong>Notify this device</strong>
              <span>
                After each check that finds new chapters, this browser shows a notification, with Mango closed too. Each device turns it on or off for itself{#if pushDevices > 0}; {pushDevices}
                  {pushDevices === 1 ? 'device has' : 'devices have'} it on now{/if}.
              </span>
            </div>
            {#if pushUnable}
              <p class="muted">{pushUnable}</p>
            {:else}
              <div class="how">
                {@render toggle(pushOn, 'Notify this device', flipPush)}
                <button class="btn" disabled={!pushDevices || pushBusy} onclick={testPush}>Send a test</button>
              </div>
            {/if}
            {#if pushNote}<p class="muted" role="status">{pushNote}</p>{/if}
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
          <div class="set">
            <div class="what">
              <strong>Export my files</strong>
              <span>
                Every downloaded chapter in one .zip, a folder for each title. Unzip it into the local folder of Mihon, Suwayomi or Mango to read them
                there.
                {#if files?.chapters}{counted(files.chapters, 'chapter')} of {counted(files.titles, 'title')}, {sized(files.bytes)}.{:else if files}Nothing
                  is downloaded yet.{/if}
              </span>
            </div>
            <div class="how">
              {#if files?.chapters}
                <a class="btn" href="api/export/files" download><Icon name="upload" size={18} />Export files</a>
              {:else}
                <button class="btn" disabled><Icon name="upload" size={18} />Export files</button>
              {/if}
            </div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Cached pages</strong>
              <span>
                Pages kept from reading online and browsing{cachedBytes ? `, ${sized(cachedBytes)}` : ''}. Mango keeps this under its own cap and fetches
                them again when needed.{#if session.account} It does not count toward your storage.{/if}
              </span>
            </div>
            <div class="how">
              <button class="btn" disabled={!cachedBytes} onclick={clearCache}><Icon name="refresh" size={18} />Clear cache</button>
            </div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Chapters already read</strong>
              <span>
                {#if readFiles?.chapters}
                  {counted(readFiles.chapters, 'saved chapter')} read already, {sized(readFiles.bytes)}. Deleting their files keeps them in the library, read, and
                  they can be read online or downloaded again.
                {:else}
                  None: every saved chapter is still unread.
                {/if}
              </span>
            </div>
            <div class="how">
              <button class="btn" disabled={!readFiles?.chapters} onclick={clearRead}><Icon name="trash" size={18} />{clearingRead ? 'Really delete them?' : 'Delete those files'}</button>
            </div>
          </div>
          <div class="set">
            <div class="what">
              <strong>Files of titles you left</strong>
              <span>
                {#if orphans.length}
                  {counted(orphans.length, 'title')} no longer in the library still {orphans.length === 1 ? 'has' : 'have'} chapters saved:
                  {orphans.map((o) => `${o.title} (${counted(o.chapters, 'chapter')}, ${sized(o.bytes)})`).join(', ')}. Their reading progress stays.
                {:else}
                  None: every saved chapter belongs to a title in the library.
                {/if}
              </span>
            </div>
            <div class="how">
              <button class="btn" disabled={!orphans.length} onclick={clearOrphans}><Icon name="trash" size={18} />{clearing ? 'Really delete them?' : 'Delete those files'}</button>
            </div>
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
            <div class="what"><strong>Extensions and repositories</strong><span>Install, update and remove the extensions that fetch titles.</span></div>
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
          {#if lacks('opds')}
            <div class="set">
              <div class="what">
                <strong>OPDS catalog</strong>
                <span>An app that reads OPDS catalogs can browse this library and fetch its chapters. Your plan leaves it out.</span>
              </div>
              <div class="how"><Locked /></div>
            </div>
          {:else}
            <div class="set stack">
              <div class="what">
                <strong>OPDS catalog</strong>
                <span>
                  An app that reads OPDS catalogs can browse this library and fetch its chapters. Give it this
                  address{#if session.login}, with your name and your password or an app password{/if}.
                </span>
              </div>
              <code>{home}opds</code>
            </div>
            {#if session.login}
              <div class="set stack">
                <div class="what">
                  <strong>KOReader progress sync</strong>
                  <span>
                    KOReader keeps the place in chapters it fetched from the catalog in step with Mango, both ways. In KOReader, open Progress sync, choose
                    Custom sync server and give it this address, then sign in with your name and an app password from Settings › Account.
                  </span>
                </div>
                <code>{home}opds/kosync</code>
              </div>
            {/if}
            <div class="set">
              <div class="what">
                <strong>Keep the place from reading apps</strong>
                <span>A page an app shows moves the chapter on, and its last page marks it read. Apps that fetch pages ahead may mark a chapter read a little early.</span>
              </div>
              <div class="how">{@render toggle(pref('opds.progress', true), 'Keep the place from reading apps', () => setPref('opds.progress', !pref('opds.progress', true)).catch((e) => (error = message(e))))}</div>
            </div>
          {/if}
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
          <div class="set">
            <div class="what"><strong>Back up by itself</strong><span>The same file, made on a schedule and kept on this server. The oldest go as new ones come.</span></div>
            <div class="how">
              <select class="btn" aria-label="How often to back up" value={String(pref('backup.hours', 0))} onchange={(e) => setPref('backup.hours', Number(e.currentTarget.value))}>
                <option value="0">Never</option>
                <option value="24">Once a day</option>
                <option value="72">Every 3 days</option>
                <option value="168">Once a week</option>
              </select>
              <select class="btn" aria-label="How many backups to keep" value={String(pref('backup.keep', 7))} onchange={(e) => setPref('backup.keep', Number(e.currentTarget.value))}>
                {#each [3, 7, 14, 30] as n (n)}<option value={String(n)}>Keep {n}</option>{/each}
              </select>
              <button class="btn" disabled={backingUp} onclick={backUpNow}>{backingUp ? 'Backing up…' : 'Back up now'}</button>
            </div>
          </div>
          {#if backups.length}
            <ul class="rows">
              {#each backups as backup (backup.name)}
                <li>
                  <span class="main"><strong>{new Date(backup.madeAt * 1000).toLocaleString()}</strong><span>{sized(backup.bytes)}</span></span>
                  <span class="tools">
                    <a class="btn" href="api/backups/{backup.name}" download><Icon name="download" size={18} />Download</a>
                    <button class="btn icon" aria-label="Delete the backup of {new Date(backup.madeAt * 1000).toLocaleString()}" onclick={() => removeBackup(backup.name)}><Icon name="trash" /></button>
                  </span>
                </li>
              {/each}
            </ul>
          {/if}
        </section>
      {:else if tab.id === 'trackers'}
        {#if lacks('trackers')}
          <section class="card">
            <h3>Your plan</h3>
            <div class="set">
              <div class="what">
                <strong>Trackers are not part of your plan</strong>
                <span>Signing in to a tracker and linking titles open with a plan that has them. A tracker signed in to already can still be left.</span>
              </div>
              <div class="how"><Locked /></div>
            </div>
          </section>
        {/if}
        <section class="card">
          <h3>Both ways</h3>
          <div class="set">
            <div class="what">
              <strong>Take in what was read elsewhere</strong>
              <span>
                After each check for new chapters, every linked title asks its trackers how far it was read, and marks those chapters read here too. Refreshing a link
                does the same at once.
              </span>
            </div>
            <div class="how">{@render toggle(pref('trackers.pull', false), 'Take in what was read elsewhere', () => flip('trackers.pull'))}</div>
          </div>
        </section>
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
            {:else if lacks('trackers')}
              <div class="set">
                <div class="what"><strong>Not signed in</strong><span>Signing in to {t.name} is not part of your plan.</span></div>
                <div class="how"><Locked /></div>
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
        {#if session.account}
          <section class="card">
            <h3>Plan and billing</h3>
            <div class="set">
              <div class="what">
                <strong>{plan.is ? `${plan.is.name} plan` : 'Your account'}</strong>
                <span>Your plan, billing and your email are kept on your account page.</span>
              </div>
              <div class="how"><a class="btn spot" href={session.account}>Manage your account</a></div>
            </div>
            {#if plan.is?.storage}
              <div class="set stack">
                <div class="what">
                  <strong>Storage</strong>
                  <span>
                    What your downloads and imports take. Cached pages do not count.
                    {#if plan.is.full}It is full: new downloads wait until some files go, or the plan grows.{/if}
                  </span>
                </div>
                <Storage storage={plan.is.storage} full={plan.is.full} />
                {#if plan.is.full}
                  <div class="how">
                    <a class="btn ink" href={session.account}><Icon name="up" size={18} stroke={3} />Get more room</a>
                    <a class="btn" href="#/settings/downloads"><Icon name="trash" size={18} />Free up space</a>
                  </div>
                {/if}
              </div>
            {/if}
            {#if leftOut.length}
              <div class="set">
                <div class="what">
                  <strong>Left out of this plan</strong>
                  <span>{leftOut.join(', ')}.</span>
                </div>
                <div class="how"><Locked label="See the plans" /></div>
              </div>
            {/if}
          </section>
        {/if}
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

          {#if !session.account}
            <section class="card">
              <h3>Two-factor authentication (2FA)</h3>
              {#if twoStep?.on}
                <div class="set stack">
                  <div class="what">
                    <strong>On</strong>
                    <span>
                      Signing in takes a code from your authenticator app as well as the password. {twoStep.codesLeft}
                      {twoStep.codesLeft === 1 ? 'recovery code is' : 'recovery codes are'} left. Reading apps sign in with an app password.
                    </span>
                  </div>
                </div>
                <form class="set" onsubmit={newRecovery}>
                  <input class="sr" type="text" name="username" autocomplete="username" value={session.name ?? 'mango'} tabindex="-1" aria-hidden="true" readonly />
                  <label class="field grow">
                    <span class="sr">Your password</span>
                    <input type="password" bind:value={twoStepPassword} placeholder="Password" autocomplete="current-password" required />
                  </label>
                  <button class="btn">New recovery codes</button>
                  <button class="btn" type="button" onclick={leaveTwoStep}>Turn off</button>
                </form>
              {:else if enrolment}
                <div class="set stack">
                  <div class="what">
                    <strong>Add Mango to your authenticator app</strong>
                    <span>Scan the picture with the app, open the link on the phone that has it, or type the key in. Then enter the six digits it shows.</span>
                  </div>
                  <!-- The picture is drawn here from Mango's own link, never from what a page sent. -->
                  <div class="qr">{@html enrolment.picture}</div>
                  <a href={enrolment.uri}>Open in the authenticator app</a>
                  <Secret values={[enrolment.secret]} label="The key, to type into the app" />
                </div>
                <form class="set" onsubmit={confirmTwoStep}>
                  <label class="field grow">
                    <span class="sr">The code the app shows</span>
                    <input type="text" bind:value={twoStepCode} placeholder="Six digits" inputmode="numeric" autocomplete="one-time-code" required />
                  </label>
                  <button class="btn ink">Turn on</button>
                  <button class="btn" type="button" onclick={() => (enrolment = null)}>Cancel</button>
                </form>
              {:else}
                <div class="set">
                  <div class="what">
                    <strong>Off</strong>
                    <span>With it on, signing in takes a code from an authenticator app on your phone as well as the password.</span>
                  </div>
                  <div class="how"><button class="btn" disabled={!twoStep} onclick={startTwoStep}>Set up</button></div>
                </div>
              {/if}
              {#if recovery.length}
                <div class="set stack">
                  <div class="what">
                    <strong>Recovery codes</strong>
                    <span>Each one signs you in once without the phone. Keep them somewhere safe: Mango shows them only now.</span>
                  </div>
                  <Secret
                    values={recovery}
                    label="Recovery codes"
                    file="mango-recovery-codes.txt"
                    heading="Mango recovery codes for {session.name ?? 'your account'}. Each one signs in once, in place of a code from the authenticator app."
                  />
                </div>
              {/if}
              {#if twoStepError}<div class="set stack"><Notice error={twoStepError} /></div>{/if}
            </section>
          {/if}

          <section class="card">
            <h3>App passwords</h3>
            <div class="set stack">
              <div class="what">
                <span>
                  A reading app signs in with your name and one of these in place of your password: over OPDS, and for KOReader's progress sync. Revoking one
                  signs out only the app that uses it.
                </span>
              </div>
            </div>
            {#if madeApp?.password}
              <div class="set stack">
                <div class="what"><strong>The password for {madeApp.name}</strong><span>Type it into the app now: Mango shows it only this once.</span></div>
                <Secret values={[madeApp.password]} label="The password for {madeApp.name}" />
              </div>
            {/if}
            {#if apps.length}
              <ul class="rows">
                {#each apps as app (app.id)}
                  <li>
                    <span class="main"><strong>{app.name}</strong><span>Made {ago(app.createdAt)} · {app.usedAt ? `last used ${ago(app.usedAt)}` : 'not used yet'}</span></span>
                    <span class="tools"><button class="btn" onclick={() => removeApp(app.id)} title="The app that uses this password can no longer sign in">Revoke</button></span>
                  </li>
                {/each}
              </ul>
            {/if}
            <form class="set" onsubmit={addApp}>
              <label class="field grow">
                <span class="sr">What the password is for</span>
                <input type="text" bind:value={appName} placeholder="What it is for, such as KOReader" autocomplete="off" maxlength="80" required />
              </label>
              <button class="btn ink">Make a password</button>
            </form>
            {#if appsError}<div class="set stack"><Notice error={appsError} /></div>{/if}
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
                    Each account has its own repositories and extensions, library, reading progress, downloads and settings. An admin is one who
                    may change this list.
                  </span>
                </div>
              </div>
              {#each users as user (user.id)}
                <div class="set">
                  <div class="what">
                    <strong>{user.name}</strong>
                    <span>{user.admin ? 'Admin' : 'Reader'}{user.name === session.name ? ' · you' : ''}{user.twoStep ? ' · 2FA' : ''} · added {ago(user.createdAt)}</span>
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
                      {#if user.twoStep}
                        <!-- For a holder who lost both the phone and the recovery codes. -->
                        <button class="btn" onclick={() => withUsers(() => send('PUT', `/users/${user.id}`, { twoStep: false }))}>Turn off 2FA</button>
                      {/if}
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
                      <button class="btn" onclick={() => removeInvite(invite.id)}>{invite.usedAt ? 'Forget' : 'Revoke'}</button>
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
          <h3>Tour</h3>
          <div class="set">
            <div class="what">
              <strong>Show the tour again</strong>
              <span>The cards a new account sees first, and where the reader's tap zones are the next time a chapter opens.</span>
            </div>
            <div class="how"><button class="btn" onclick={againTour}>Show the tour</button></div>
          </div>
        </section>

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

  /* An authenticator scans dark on light, whatever the theme. */
  .qr {
    width: 196px;
    max-width: 100%;
    padding: 6px;
    border: 3px solid var(--ink);
    background: #fff;
  }

  .qr :global(svg) {
    display: block;
    width: 100%;
    height: auto;
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
