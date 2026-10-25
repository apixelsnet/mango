<script lang="ts" module>
  // Auto-scroll that ran off the end of a chapter carries on in the next one.
  let carry = false;
</script>

<script lang="ts">
  import { untrack } from 'svelte';
  import { get, message, send, type PageMark, type ReaderInfo } from '../lib/api';
  import { display } from '../lib/display.svelte';
  import Icon from '../lib/Icon.svelte';
  import { makeOver, type Part } from '../lib/pages';
  import ReaderPrefs from '../lib/ReaderPrefs.svelte';
  import {
    filterOf,
    FITS,
    keymap,
    keyName,
    KINDS,
    merge,
    MODES,
    owned,
    profile,
    setProfile,
    sharpenKernel,
    zone,
    type Kind,
    type Mode,
    type Own,
    type Prefs,
  } from '../lib/reader';
  import { pref, prefs, setPref } from '../lib/prefs.svelte';
  import { go, route, up } from '../lib/router.svelte';
  import Zones from '../lib/Zones.svelte';

  let { id }: { id: number } = $props();

  let info = $state<ReaderInfo | null>(null);
  let count = $state(0);
  let index = $state(0);
  let ready = $state(false);
  let zoom = $state(1);
  let overlay = $state(false);
  let panel = $state(false);
  // Where taps turn the page, drawn over it: by itself the first time an account reads, and when asked.
  let zones = $state(false);
  let error = $state<string | null>(null);
  let note = $state('');
  let flash = $state('');
  let full = $state(false);
  let broken = $state<Record<number, boolean>>({});
  let tries = $state<Record<number, number>>({});
  // Webtoon only: which pages are close enough to be in the document, and their shapes once seen.
  let near = $state<Record<number, boolean>>({});
  let shapes = $state<Record<number, string>>({});
  // Pages wider than tall: a spread already, never paired with another.
  let wide = $state<Record<number, boolean>>({});
  let stage = $state<HTMLElement>();
  let strip = $state<HTMLElement>();
  let vw = $state(0);
  let vh = $state(0);
  // A wide page split in two: which half is up, the first in reading order or the second.
  let half = $state<0 | 1>(0);
  // Pages made over in the browser, cropped or halved: their blob addresses by page, part and setting.
  let views = $state<Record<string, string>>({});
  // Pages kept with a note, and the note being written for the page that is up.
  let marks = $state<PageMark[]>([]);
  let noting = $state<string | null>(null);
  // Nothing of this reading is kept: no place, no history, nothing for trackers.
  const incognito = $derived(display.value.incognito);

  // ---------- settings: the kind of series, then what this title says for itself ----------

  let own = $state<Own>({});
  let scope = $state<'title' | 'kind'>('title');
  const kind = $derived<Kind>(own.kind ?? info?.kind ?? 'manga');
  const p = $derived<Prefs>({ ...profile(kind), ...own });
  const mode = $derived(p.mode);
  const fit = $derived(p.fit);
  const speed = $derived(p.autoSpeed);
  const seconds = $derived(p.autoSeconds);
  const awake = $derived(p.awake);

  $effect(() => {
    if (ready && prefs.ready && p.zones !== 'off' && !pref('tour.zones', false)) untrack(() => (zones = true));
  });

  function closeZones() {
    zones = false;
    setPref('tour.zones', true).catch(() => {});
  }

  const paged = $derived(mode !== 'webtoon' && mode !== 'sideways');
  const sideways = $derived(mode === 'sideways');
  const keys = $derived(keymap());
  const pages = $derived(Array.from({ length: count }, (_, i) => i));
  const two = $derived((mode === 'rtl' || mode === 'ltr') && (p.spread === 'on' || (p.spread === 'auto' && vw > vh * 1.2)));
  /** With two pages up: which pages share the screen, in reading order. */
  const leaves = $derived.by(() => {
    if (!two) return null;
    const out: number[][] = [];
    let i = 0;
    if (p.offset && count) out.push([i++]);
    while (i < count) {
      if (wide[i] || wide[i + 1] || i + 1 >= count) out.push([i++]);
      else out.push([i++, i++]);
    }
    return out;
  });
  const shown = $derived(leaves?.find((leaf) => leaf.includes(index)) ?? [index]);
  /** The last page on screen: how far the reading got. */
  const reached = $derived(shown[shown.length - 1]);
  const marked = $derived(marks.find((mark) => mark.page === reached) ?? null);

  /** Whether a page is read as its two halves. Only one page at a time is split, and only a wide one. */
  const splitting = (i: number) => paged && p.split && !two && !!wide[i];
  const partOf = (i: number): Part => (splitting(i) ? (half === 0 ? 'first' : 'second') : 'whole');
  const viewKey = (i: number, part: Part) => `${i}:${part}:${p.crop}:${mode === 'rtl'}:${tries[i] ?? 0}`;
  /** A page as it is shown: made over when it is ready, the page itself until then. */
  const look = (i: number, part: Part = partOf(i)) => views[viewKey(i, part)] ?? src(i);

  function prepare(i: number, part: Part) {
    const key = viewKey(i, part);
    if (views[key]) return;
    makeOver(src(i), p.crop, part, mode === 'rtl').then((url) => url && (views[key] = url));
  }

  $effect(() => {
    if (!ready || !(p.crop || p.split)) return;
    const wanted: [number, Part][] = [];
    if (paged) {
      for (const i of [...shown, reached + 1]) if (i >= 0 && i < count) wanted.push([i, partOf(i)]);
      // The other half too, so turning to it is quick.
      if (splitting(index)) wanted.push([index, half === 0 ? 'second' : 'first']);
    } else {
      for (const i of pages) if (near[i]) wanted.push([i, 'whole']);
    }
    untrack(() => wanted.forEach(([i, part]) => prepare(i, part)));
  });

  const src = (i: number, chapter = id) => `api/chapters/${chapter}/pages/${i}${tries[i] ? `?try=${tries[i]}` : ''}`;
  const clamp = (z: number) => Math.min(4, Math.max(0.5, z));
  const between = (value: number, low: number, high: number) => Math.min(high, Math.max(low, value));

  let keeping: ReturnType<typeof setTimeout> | undefined;
  let unkept = false;

  function remember(now = false) {
    clearTimeout(keeping);
    const write = () => {
      unkept = false;
      if (!info) return;
      const { mode, fit, ...prefs } = own;
      send('PUT', `/manga/${info.mangaId}/reader`, { mode: mode ?? null, fit: fit ?? null, prefs }).catch(() => {});
    };
    if (now) write();
    else {
      unkept = true;
      keeping = setTimeout(write, 300);
    }
  }

  /** A setting changed: for this title, or for every title of its kind. */
  function change(patch: Partial<Prefs>) {
    if ('mode' in patch || 'fit' in patch || 'spread' in patch) zoom = 1;
    if (scope === 'kind') {
      setProfile(kind, patch);
      // Or the title's own answer would hide the change just made.
      own = merge(own, Object.fromEntries(Object.keys(patch).map((key) => [key, undefined])));
    } else own = merge(own, patch);
    remember();
  }

  function setKind(next: Kind) {
    own = merge(own, { kind: next === info?.kind ? undefined : next });
    remember();
  }

  function forget() {
    own = {};
    remember();
  }

  let flashing: ReturnType<typeof setTimeout> | undefined;

  /** A word on screen for a moment, for what the keys change while the controls are away. */
  function say(text: string) {
    flash = text;
    clearTimeout(flashing);
    flashing = setTimeout(() => (flash = ''), 1400);
  }

  // ---------- opening ----------

  /** Called when the page count is known. The first call picks the page to open on. */
  function open(total: number) {
    count = total;
    if (ready || !info) return;
    const chapter = info.chapter;
    // A kept page is opened by its number; otherwise where the reading got to.
    const asked = Number(route.query.get('page'));
    index = route.query.get('at') === 'end'
      ? total - 1
      : route.query.has('page') && Number.isInteger(asked)
        ? between(asked, 0, total - 1)
        : chapter.read
          ? 0
          : Math.min(Math.max(chapter.lastPageRead, 0), total - 1);
    saved = index;
    ready = true;
  }

  async function load() {
    // Both at once. A chapter opened before already knows its page count, and then
    // the first page shows without waiting for the source.
    const counting = get<{ count: number }>(`/chapters/${id}/pages`);
    counting.catch(() => {});
    try {
      info = await get<ReaderInfo>(`/chapters/${id}`);
      own = owned(info);
      marks = info.marks;
      fetchAhead(info);
      if (info.chapter.pageCount) open(info.chapter.pageCount);
      open((await counting).count);
    } catch (e) {
      if (!ready) error = message(e);
    }
  }
  load();

  /** Queues the next few chapters for download, as many as Settings asks, for a title in the library. */
  function fetchAhead(info: ReaderInfo) {
    const ahead = pref<number>('reader.downloadAhead', 0);
    if (!ahead || !info.inLibrary || incognito) return;
    // Newest first, so the chapters after this one are those listed above it.
    const at = info.chapters.findIndex((chapter) => chapter.id === id);
    const next = info.chapters.slice(Math.max(0, at - ahead), Math.max(0, at)).map((chapter) => chapter.id);
    if (next.length) send('POST', '/downloads', { chapters: next }).catch(() => {});
  }

  // ---------- moving ----------

  /** Opens a page, on its first half when it is split, or on its second with `last`. */
  function show(i: number, last = false) {
    index = Math.max(0, Math.min(count - 1, i));
    half = last && splitting(index) ? 1 : 0;
    zoom = 1;
    note = '';
    if (paged) stage?.scrollTo({ top: 0, left: 0 });
    else strip?.querySelector(`[data-page="${index}"]`)?.scrollIntoView({ block: 'start', inline: 'start' });
  }

  function next() {
    if (splitting(index) && half === 0) {
      half = 1;
      stage?.scrollTo({ top: 0, left: 0 });
    } else if (reached < count - 1) show(reached + 1);
    else if (info?.next) go(`/read/${info.next}`, true);
    else {
      note = 'That was the last chapter.';
      overlay = true;
    }
  }

  function prev() {
    if (splitting(index) && half === 1) {
      half = 0;
      stage?.scrollTo({ top: 0, left: 0 });
    } else if (shown[0] > 0) show(shown[0] - 1, true);
    else if (info?.prev) go(`/read/${info.prev}?at=end`, true);
    else note = 'This is the first chapter.';
  }

  /** Can the page still scroll that way inside its frame? */
  function room(axis: 'x' | 'y', direction: number): boolean {
    const box = paged ? stage : strip;
    if (!box) return false;
    const [at, size, view] = axis === 'x' ? [box.scrollLeft, box.scrollWidth, box.clientWidth] : [box.scrollTop, box.scrollHeight, box.clientHeight];
    return direction > 0 ? at + view < size - 2 : at > 2;
  }

  /** Space and its kin: scroll within a tall page first, turn at its end. A sideways strip scrolls across. */
  function advance(direction: 1 | -1) {
    const box = paged ? stage : strip;
    if (sideways) box?.scrollBy({ left: ((box.clientWidth * p.step) / 100) * direction, behavior: 'smooth' });
    else if (!paged || room('y', direction)) box?.scrollBy({ top: ((box.clientHeight * p.step) / 100) * direction, behavior: 'smooth' });
    else if (direction > 0) next();
    else prev();
  }

  // ---------- kept pages ----------

  /** Keeps the page that is up with the note written, or lets it go with null. */
  async function keepPage(text: string | null) {
    const page = reached;
    noting = null;
    try {
      if (text === null) {
        await send('DELETE', `/chapters/${id}/marks/${page}`);
        marks = marks.filter((mark) => mark.page !== page);
        say('Page let go');
      } else {
        await send('PUT', `/chapters/${id}/marks/${page}`, { note: text });
        const kept = { chapterId: id, chapterName: info?.chapter.name ?? '', page, note: text.trim(), createdAt: Date.now() / 1000 };
        marks = [...marks.filter((mark) => mark.page !== page), kept];
        say('Page kept');
      }
    } catch (e) {
      say(message(e));
    }
  }

  // ---------- remembering ----------

  let saved = -1;
  let saving: ReturnType<typeof setTimeout> | undefined;

  function save(now: boolean) {
    clearTimeout(saving);
    const page = reached;
    if (!ready || page === saved || incognito) return;
    const write = () => {
      saved = page;
      const body = page >= count - 1 ? { page, read: true } : { page };
      // keepalive lets the request finish while the page is closing.
      fetch(`api/chapters/${id}/progress`, { method: 'PUT', keepalive: true, headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(body) }).catch(() => {});
    };
    if (now) write();
    else saving = setTimeout(write, 400);
  }

  $effect(() => {
    void reached;
    if (ready) save(false);
  });
  $effect(() => () => {
    save(true);
    if (unkept) remember(true);
  });

  // ---------- fetching ahead ----------

  /** A page's shape is known once it has loaded, here or ahead of time. */
  function seen(i: number, image: HTMLImageElement) {
    if (image.naturalWidth > image.naturalHeight && !wide[i]) wide[i] = true;
  }

  let warmed = false;
  $effect(() => {
    if (!ready) return;
    if (paged) {
      const ahead = Array.from({ length: p.ahead }, (_, k) => reached + 1 + k);
      for (const i of [...ahead, shown[0] - 1]) {
        if (i < 0 || i >= count) continue;
        const image = new Image();
        image.onload = () => seen(i, image);
        image.src = src(i);
      }
    }
    const after = info?.next;
    if (after && !warmed && reached >= count - 3) {
      warmed = true;
      get(`/chapters/${after}/pages`)
        .then(() => [0, 1].forEach((i) => (new Image().src = `api/chapters/${after}/pages/${i}`)))
        .catch(() => {});
    }
  });

  // ---------- webtoon: only nearby images exist ----------

  $effect(() => {
    if (!ready || paged || !strip || !count) return;
    const box = strip;
    const across = sideways;
    const page = (entry: IntersectionObserverEntry) => Number((entry.target as HTMLElement).dataset.page);
    const loader = new IntersectionObserver((entries) => entries.forEach((e) => (near[page(e)] = e.isIntersecting)), {
      root: box,
      rootMargin: across ? '0px 250%' : '250% 0px',
    });
    // The page crossing the middle of the screen is the one being read.
    const tracker = new IntersectionObserver((entries) => entries.forEach((e) => e.isIntersecting && (index = page(e))), {
      root: box,
      rootMargin: across ? '0px -50% 0px -50%' : '-50% 0px -50% 0px',
    });
    for (const frame of box.querySelectorAll('[data-page]')) {
      loader.observe(frame);
      tracker.observe(frame);
    }
    untrack(() => box.querySelector(`[data-page="${index}"]`)?.scrollIntoView({ block: 'start', inline: 'start' }));
    return () => {
      loader.disconnect();
      tracker.disconnect();
    };
  });

  function measured(i: number, image: HTMLImageElement) {
    if (image.naturalWidth) shapes[i] = `${image.naturalWidth} / ${image.naturalHeight}`;
  }

  function retry(i: number) {
    tries[i] = (tries[i] ?? 0) + 1;
    broken[i] = false;
  }

  // ---------- auto-scroll ----------

  let auto = $state(carry);
  carry = false;
  // A finger resting on the page holds it still.
  let held = false;

  /** The chapter ran out under auto-scroll. */
  function autoEnd() {
    if (p.autoNext && info?.next) {
      carry = true;
      go(`/read/${info.next}`, true);
    } else {
      auto = false;
      note = info?.next ? '' : 'That was the last chapter.';
      overlay = true;
    }
  }

  $effect(() => {
    // The controls being up is a pause.
    if (!auto || !ready || overlay) return;
    if (paged) {
      const turn = () => {
        if (document.hidden || held) return;
        if (reached < count - 1 || room('y', 1)) advance(1);
        else autoEnd();
      };
      const timer = setInterval(turn, seconds * 1000);
      return () => clearInterval(timer);
    }
    const box = strip;
    if (!box) return;
    const pace = speed;
    // A sideways strip runs across, the long strip down.
    const across = sideways;
    const position = () => (across ? box.scrollLeft : box.scrollTop);
    const move = (to: number) => (across ? (box.scrollLeft = to) : (box.scrollTop = to));
    const atEnd = () => (across ? box.scrollLeft + box.clientWidth >= box.scrollWidth - 2 : box.scrollTop + box.clientHeight >= box.scrollHeight - 2);
    let at = position();
    let last = performance.now();
    let rested = 0;
    let frame = 0;
    const tick = (now: number) => {
      const passed = Math.min(now - last, 100) / 1000;
      last = now;
      frame = requestAnimationFrame(tick);
      if (held) return;
      // Scrolled by hand meanwhile: carry on from there.
      if (Math.abs(position() - at) > 2) at = position();
      at += pace * passed;
      move(at);
      if (!atEnd()) rested = 0;
      else if ((rested += passed) > 1.5) {
        rested = -Infinity;
        autoEnd();
      }
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  });

  function toggleAuto() {
    auto = !auto;
    if (auto) overlay = panel = false;
    say(auto ? 'Auto-scroll on' : 'Auto-scroll off');
  }

  function pace(direction: 1 | -1) {
    if (paged) {
      const next = between(p.autoSeconds - direction, 2, 60);
      change({ autoSeconds: next });
      say(`A page every ${next} s`);
    } else {
      const next = between(Math.round((p.autoSpeed * (direction > 0 ? 1.25 : 0.8)) / 10) * 10, 20, 600);
      change({ autoSpeed: next });
      say(`Auto-scroll ${next} px/s`);
    }
  }

  // ---------- the screen ----------

  $effect(() => {
    if (!awake || !('wakeLock' in navigator)) return;
    let lock: WakeLockSentinel | undefined;
    let over = false;
    // The browser lets go of it whenever the tab is hidden.
    const take = () => {
      if (document.visibilityState !== 'visible') return;
      navigator.wakeLock
        .request('screen')
        .then((got) => {
          if (over) return got.release();
          lock = got;
        })
        .catch(() => {});
    };
    take();
    document.addEventListener('visibilitychange', take);
    return () => {
      over = true;
      document.removeEventListener('visibilitychange', take);
      lock?.release().catch(() => {});
    };
  });

  function fullscreen() {
    (document.fullscreenElement ? document.exitFullscreen() : document.documentElement.requestFullscreen()).catch(() => {});
  }

  // ---------- input ----------

  function tap(event: MouseEvent) {
    if ((event.target as HTMLElement).closest('button, a, input, select')) return;
    if (panel) {
      panel = false;
      return;
    }
    const box = (paged ? stage : strip)?.getBoundingClientRect();
    if (box) {
      const hit = zone(p, (event.clientX - box.left) / box.width, (event.clientY - box.top) / box.height);
      if (hit !== 'menu') {
        if (!paged) advance(hit === 'next' ? 1 : -1);
        else if (hit === 'next') next();
        else prev();
        return;
      }
    }
    overlay = !overlay;
  }

  // A swipe turns the page only when it did not scroll anything.
  let touch: { x: number; y: number; left: number; top: number } | null = null;

  function touchstart(event: TouchEvent) {
    held = true;
    const one = event.touches.length === 1 && stage ? event.touches[0] : null;
    touch = one && stage ? { x: one.clientX, y: one.clientY, left: stage.scrollLeft, top: stage.scrollTop } : null;
  }

  function touchend(event: TouchEvent) {
    held = event.touches.length > 0;
    if (!touch || !stage || !paged) return;
    const end = event.changedTouches[0];
    const [dx, dy] = [end.clientX - touch.x, end.clientY - touch.y];
    const scrolled = stage.scrollLeft !== touch.left || stage.scrollTop !== touch.top;
    touch = null;
    if (scrolled) return;
    if (mode === 'vertical') {
      if (Math.abs(dy) > 70 && Math.abs(dy) > Math.abs(dx) * 1.5) (dy < 0 ? next : prev)();
    } else if (Math.abs(dx) > 70 && Math.abs(dx) > Math.abs(dy) * 1.5) {
      const forward = mode === 'rtl' ? dx > 0 : dx < 0;
      (forward ? next : prev)();
    }
  }

  let wheeled = 0;
  let wheelRest = 0;

  function wheel(event: WheelEvent) {
    if (event.ctrlKey) {
      event.preventDefault();
      zoom = clamp(zoom * (event.deltaY < 0 ? 1.1 : 0.9));
      return;
    }
    // A mouse wheel turns a sideways strip across.
    if (sideways && strip && Math.abs(event.deltaY) > Math.abs(event.deltaX)) {
      event.preventDefault();
      strip.scrollBy({ left: event.deltaY });
      return;
    }
    if (!paged || room('y', Math.sign(event.deltaY))) return;
    const now = Date.now();
    if (now < wheelRest) return;
    wheeled += event.deltaY;
    if (Math.abs(wheeled) > 120) {
      (wheeled > 0 ? next : prev)();
      wheeled = 0;
      wheelRest = now + 350;
    }
  }

  /** What a key the owner can set does; see `ACTIONS` in lib/reader.ts. */
  function act(action: ReturnType<typeof keys.get>): boolean {
    switch (action) {
      case 'next':
        advance(1);
        break;
      case 'prev':
        advance(-1);
        break;
      case 'nextChapter':
        if (info?.next) go(`/read/${info.next}`, true);
        break;
      case 'prevChapter':
        if (info?.prev) go(`/read/${info.prev}`, true);
        break;
      case 'menu':
        overlay = !overlay;
        break;
      case 'settings':
        panel = !panel;
        overlay = false;
        break;
      case 'auto':
        toggleAuto();
        break;
      case 'faster':
        pace(1);
        break;
      case 'slower':
        pace(-1);
        break;
      case 'spread':
        if (mode === 'rtl' || mode === 'ltr') {
          change({ spread: two ? 'off' : 'on' });
          say(two ? 'One page' : 'Two pages');
        }
        break;
      case 'zoomIn':
        zoom = clamp(zoom * 1.2);
        break;
      case 'zoomOut':
        zoom = clamp(zoom / 1.2);
        break;
      case 'zoomReset':
        zoom = 1;
        break;
      case 'mark':
        keepPage(marked ? null : '');
        break;
      case 'fit': {
        const [next, label] = FITS[(FITS.findIndex(([name]) => name === fit) + 1) % FITS.length];
        change({ fit: next });
        say(label);
        break;
      }
      default:
        return false;
    }
    return true;
  }

  function key(event: KeyboardEvent) {
    if (event.metaKey || event.ctrlKey || event.altKey) return;
    const on = event.target instanceof Element ? event.target : null;
    if (event.key === 'Escape') {
      if (noting !== null) noting = null;
      else if (zones) closeZones();
      else if (panel) panel = false;
      else if (overlay) overlay = false;
      else if (info) up(`/title/${info.mangaId}`);
      event.preventDefault();
      return;
    }
    if (on?.matches('input, select, textarea')) return;
    // A button in focus keeps the keys that press it.
    if ((event.key === ' ' || event.key === 'Enter') && on?.closest('button, a')) return;
    const box = paged ? stage : strip;
    switch (event.key) {
      case 'ArrowRight':
        if (!paged) advance(1);
        else if (mode === 'rtl') prev();
        else next();
        break;
      case 'ArrowLeft':
        if (!paged) advance(-1);
        else if (mode === 'rtl') next();
        else prev();
        break;
      case 'ArrowDown':
        if (mode === 'vertical' && !room('y', 1)) next();
        else if (sideways) box?.scrollBy({ left: 120 });
        else box?.scrollBy({ top: 120 });
        break;
      case 'ArrowUp':
        if (mode === 'vertical' && !room('y', -1)) prev();
        else if (sideways) box?.scrollBy({ left: -120 });
        else box?.scrollBy({ top: -120 });
        break;
      case ' ':
        advance(event.shiftKey ? -1 : 1);
        break;
      case 'Home':
        show(0);
        break;
      case 'End':
        show(count - 1);
        break;
      default:
        if (!act(keys.get(keyName(event.key)))) return;
    }
    event.preventDefault();
  }
</script>

<svelte:window onkeydown={key} onpagehide={() => save(true)} bind:innerWidth={vw} bind:innerHeight={vh} />
<svelte:document onfullscreenchange={() => (full = !!document.fullscreenElement)} />

<div class="reader" data-ground={p.ground} data-scaling={p.scaling} style:--filter={filterOf(p)}>
  {#if p.sharpen > 0}
    <svg class="defs" aria-hidden="true" width="0" height="0">
      <filter id="mango-sharpen"><feConvolveMatrix order="3" kernelMatrix={sharpenKernel(p.sharpen)} preserveAlpha="true" /></filter>
    </svg>
  {/if}
  {#if error}
    <div class="center">
      <div class="panel message" role="alert">
        <strong>This chapter did not open.</strong>
        <span>{error}</span>
        <div class="tools">
          <button class="btn" onclick={() => ((error = null), load())}>Try again</button>
          <a class="btn" href={info ? `#/title/${info.mangaId}` : '#/'}>Back</a>
        </div>
      </div>
    </div>
  {:else if !ready}
    <div class="center" role="status">Opening…</div>
  {:else if paged}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="stage"
      data-fit={fit}
      style:--z={zoom}
      bind:this={stage}
      onclick={tap}
      ontouchstart={touchstart}
      ontouchend={touchend}
      ontouchcancel={() => ((held = false), (touch = null))}
      onwheel={wheel}
    >
      {#key shown.join()}
        <span class="number display" aria-hidden="true">{shown[0] + 1}</span>
        <div class="leaf" class:two={shown.length > 1} class:rtl={mode === 'rtl'}>
          {#each shown as i (`${i}:${tries[i] ?? 0}`)}
            {#if broken[i]}
              <div class="panel message">
                <strong>Page {i + 1} did not load.</strong>
                <button class="btn" onclick={() => retry(i)}>Try again</button>
              </div>
            {:else}
              <img
                src={look(i)}
                alt="Page {i + 1} of {count}{splitting(i) ? `, ${half === 0 ? 'first' : 'second'} half` : ''}"
                draggable="false"
                onload={(e) => look(i) === src(i) && seen(i, e.currentTarget as HTMLImageElement)}
                onerror={() => (broken[i] = true)}
              />
            {/if}
          {/each}
        </div>
      {/key}
    </div>
  {:else}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="strip"
      class:sideways
      bind:this={strip}
      onclick={tap}
      onwheel={wheel}
      ontouchstart={() => (held = true)}
      ontouchend={(e) => (held = e.touches.length > 0)}
      ontouchcancel={() => (held = false)}
    >
      {#each pages as i (i)}
        <div
          class="frame"
          data-page={i}
          style:aspect-ratio={shapes[i] ?? '2 / 3'}
          style:max-width={sideways ? null : `${p.width * zoom}px`}
          style:margin-bottom={sideways ? null : `${p.gap}px`}
          style:margin-right={sideways ? `${p.gap}px` : null}
        >
          {#if broken[i]}
            <div class="panel message">
              <strong>Page {i + 1} did not load.</strong>
              <button class="btn" onclick={() => retry(i)}>Try again</button>
            </div>
          {:else if near[i]}
            <img src={look(i, 'whole')} alt="Page {i + 1} of {count}" draggable="false" onload={(e) => measured(i, e.currentTarget as HTMLImageElement)} onerror={() => (broken[i] = true)} />
          {/if}
        </div>
      {/each}
      <div class="tail">
        {#if info?.next}
          <button class="btn big spot" onclick={next}>Next chapter<Icon name="arrow" size={18} stroke={3} /></button>
        {:else}
          <strong>That was the last chapter.</strong>
        {/if}
      </div>
    </div>
  {/if}

  {#if ready && !error}
    {#if p.tintAmount > 0}
      <div class="tint" style:background={p.tint} style:opacity={p.tintAmount / 100} style:mix-blend-mode={p.blend}></div>
    {/if}
    {#if p.bar}
      <div class="progress" class:rtl={mode === 'rtl'} aria-hidden="true"><span style:width="{((reached + 1) / count) * 100}%"></span></div>
    {/if}
    {#if p.number && !overlay && !panel}
      <span class="hud" aria-hidden="true">{shown.length > 1 ? `${shown[0] + 1}–${reached + 1}` : reached + 1} / {count}{marked ? ' · kept' : ''}</span>
    {/if}
    {#if noting !== null}
      <form class="note" onsubmit={(e) => (e.preventDefault(), keepPage(noting ?? ''))}>
        <label>
          <span>A note for page {reached + 1}</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input bind:value={noting} maxlength="2000" placeholder="What is on this page" autofocus />
        </label>
        <div class="tools">
          <button class="btn ink">Keep</button>
          {#if marked}<button class="btn" type="button" onclick={() => keepPage(null)}>Let go</button>{/if}
          <button class="btn" type="button" onclick={() => (noting = null)}>Cancel</button>
        </div>
      </form>
    {/if}
    {#if flash}<span class="flash" role="status">{flash}</span>{/if}
  {/if}

  {#if overlay && info && ready}
    <div class="bar top">
      <!-- Back to the title this was opened from, not on to a second copy of it: or its own way back would lead here again. -->
      <a class="btn icon" href="#/title/{info.mangaId}" aria-label="Back to the title" onclick={(e) => (e.preventDefault(), up(`/title/${info?.mangaId}`))}>
        <Icon name="back" stroke={3} />
      </a>
      <div class="what">
        <strong>{info.title}</strong>
        <span>{info.chapter.name}{incognito ? ' · Incognito: nothing is kept' : ''}</span>
      </div>
      <button
        class="btn icon"
        aria-label={marked ? `Page ${reached + 1} is kept: change its note` : `Keep page ${reached + 1} with a note`}
        aria-pressed={!!marked}
        onclick={() => ((noting = marked?.note ?? ''), (overlay = false))}
      >
        <Icon name="bookmark" fill={!!marked} />
      </button>
      {#if document.fullscreenEnabled}
        <button class="btn icon" aria-label="Full screen" aria-pressed={full} onclick={fullscreen}><Icon name="expand" /></button>
      {/if}
      {#if p.zones !== 'off'}
        <button class="btn icon" aria-label="Show the tap zones" onclick={() => ((zones = true), (overlay = false))}><Icon name="grid" /></button>
      {/if}
      <button class="btn icon" aria-label="Hide the controls" onclick={() => (overlay = false)}><Icon name="close" /></button>
    </div>
    <div class="bar bottom">
      {#if note}<p role="status">{note}</p>{/if}
      <div class="line">
        <button class="btn icon" aria-label="Previous chapter" disabled={!info.prev} onclick={() => go(`/read/${info?.prev}`, true)}>
          <Icon name="first" />
        </button>
        <input
          type="range"
          min="0"
          max={count - 1}
          value={index}
          dir={mode === 'rtl' ? 'rtl' : 'ltr'}
          aria-label="Page"
          oninput={(e) => show(Number(e.currentTarget.value))}
        />
        <strong class="count">{reached + 1} / {count}</strong>
        <button class="btn icon" aria-label="Next chapter" disabled={!info.next} onclick={() => go(`/read/${info?.next}`, true)}>
          <Icon name="last" />
        </button>
      </div>
      <div class="line">
        <select class="btn" aria-label="Chapter" value={id} onchange={(e) => go(`/read/${e.currentTarget.value}`, true)}>
          {#each info.chapters as chapter (chapter.id)}
            <option value={chapter.id}>{chapter.name}</option>
          {/each}
        </select>
        <select class="btn" aria-label="Reading direction" value={mode} onchange={(e) => change({ mode: e.currentTarget.value as Mode })}>
          {#each MODES as [value, label] (value)}<option {value}>{label}</option>{/each}
        </select>
        <button class="btn" aria-pressed={auto} onclick={toggleAuto}><Icon name={auto ? 'pause' : 'play'} size={16} fill />Auto</button>
        <button class="btn icon" aria-label="Reader settings" onclick={() => ((panel = true), (overlay = false))}><Icon name="sliders" /></button>
      </div>
    </div>
  {/if}

  {#if zones && info && ready}
    <Zones {p} onclose={closeZones} />
  {/if}

  {#if panel && info && ready}
    {@const entry = KINDS.find((each) => each.id === kind) ?? KINDS[0]}
    <aside class="sheet" aria-label="Reader settings">
      <header>
        <strong class="display">READER</strong>
        <button class="btn icon" aria-label="Close the settings" onclick={() => (panel = false)}><Icon name="close" /></button>
      </header>
      <div class="scroll">
        <div class="pick">
          <label>
            <span>Read as</span>
            <select class="btn" value={kind} onchange={(e) => setKind(e.currentTarget.value as Kind)}>
              {#each KINDS as each (each.id)}<option value={each.id}>{each.name}{each.id === info.kind ? ' (as found)' : ''}</option>{/each}
            </select>
          </label>
          <div class="seg" role="group" aria-label="Where a change is kept">
            <button aria-pressed={scope === 'title'} onclick={() => (scope = 'title')}>This title</button>
            <button aria-pressed={scope === 'kind'} onclick={() => (scope = 'kind')}>{entry.all}</button>
          </div>
          <p>
            {scope === 'title' ? 'A change is kept for this title alone.' : `A change is kept for ${entry.all.toLowerCase()}, this title included.`}
          </p>
        </div>
        <ReaderPrefs value={p} onchange={change} />
        <button class="btn" disabled={!Object.keys(own).length} onclick={forget}>Forget what this title keeps for itself</button>
      </div>
    </aside>
  {/if}
</div>

<style>
  /* The reader keeps a dark ground of its own unless told otherwise: pages are what is lit. */
  /* Opened from a phone's home screen it starts below the clock, which would lie over the page. */
  .reader {
    position: fixed;
    inset: env(safe-area-inset-top, 0px) 0 0;
    background: #111111;
    color: #f1f1ec;
  }

  .reader[data-ground='grey'] {
    background: #55554f;
  }

  .reader[data-ground='white'] {
    background: #ffffff;
    color: #111111;
  }

  .reader[data-ground='theme'] {
    background: var(--paper);
    color: var(--ink);
  }

  .center {
    display: flex;
    align-items: center;
    justify-content: center;
    height: 100%;
    padding: 24px;
    font-weight: 700;
  }

  .message {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 12px;
    max-width: 440px;
    margin: auto;
    padding: 20px;
    color: var(--ink);
  }

  .message span {
    overflow-wrap: anywhere;
  }

  .stage {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    overflow: auto;
    overscroll-behavior: contain;
  }

  /* One page, or two side by side. Each fit sizes it from the screen times the zoom. */
  .leaf {
    position: relative;
    display: flex;
    flex: none;
    align-items: center;
    margin: auto;
  }

  .leaf.rtl {
    flex-direction: row-reverse;
  }

  .leaf img {
    display: block;
    flex: none;
    filter: var(--filter);
    user-select: none;
  }

  .stage[data-fit='height'] img {
    max-width: none;
    height: calc((100dvh - env(safe-area-inset-top, 0px)) * var(--z));
  }

  .stage[data-fit='width'] .leaf {
    width: calc(100% * var(--z));
  }

  .stage[data-fit='width'] img {
    width: 100%;
    height: auto;
  }

  .stage[data-fit='screen'] .leaf {
    width: calc(100% * var(--z));
    height: calc(100% * var(--z));
  }

  .stage[data-fit='screen'] img {
    width: 100%;
    height: 100%;
    object-fit: contain;
  }

  .stage[data-fit='width'] .two img,
  .stage[data-fit='screen'] .two img {
    width: 50%;
  }

  /* Two pages meet at the spine, whichever side each is on. */
  .stage[data-fit='screen'] .two img:first-child {
    object-position: right center;
  }

  .stage[data-fit='screen'] .two img:last-child {
    object-position: left center;
  }

  .stage[data-fit='screen'] .two.rtl img:first-child {
    object-position: left center;
  }

  .stage[data-fit='screen'] .two.rtl img:last-child {
    object-position: right center;
  }

  .stage[data-fit='original'] img {
    zoom: var(--z);
  }

  /* The page number waits behind the image, so a slow page is not a blank screen. */
  .number {
    position: absolute;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 96px;
    opacity: 0.18;
    pointer-events: none;
  }

  .strip {
    height: 100%;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .frame {
    display: flex;
    width: 100%;
    margin: 0 auto;
  }

  .frame img {
    display: block;
    width: 100%;
    height: 100%;
    filter: var(--filter);
    user-select: none;
  }

  /* The sideways strip: pages side by side at the screen's height, scrolled across. */
  .strip.sideways {
    display: flex;
    align-items: stretch;
    overflow-x: auto;
    overflow-y: hidden;
  }

  .sideways .frame {
    flex: none;
    width: auto;
    height: 100%;
    margin: 0;
  }

  .sideways .tail {
    flex: none;
    align-items: center;
    padding: 16px 48px;
  }

  .reader[data-scaling='crisp'] img {
    image-rendering: crisp-edges;
  }

  .reader[data-scaling='pixelated'] img {
    image-rendering: pixelated;
  }

  .defs {
    position: absolute;
  }

  .note {
    position: absolute;
    top: 50%;
    left: 50%;
    z-index: 3;
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(440px, calc(100% - 32px));
    padding: 16px;
    border: 3px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
    box-shadow: var(--sh-5);
    transform: translate(-50%, -50%);
  }

  .note label {
    display: flex;
    flex-direction: column;
    gap: 8px;
    font-weight: 900;
  }

  .note input {
    min-height: 44px;
    padding: 0 12px;
    border: 3px solid var(--ink);
    background: var(--surface);
    color: var(--ink);
    font: inherit;
  }

  .tail {
    display: flex;
    justify-content: center;
    padding: 48px 16px 96px;
  }

  /* ---------- over the pages ---------- */

  .tint {
    position: absolute;
    inset: 0;
    z-index: 1;
    pointer-events: none;
  }

  .progress {
    position: absolute;
    right: 0;
    bottom: 0;
    left: 0;
    z-index: 1;
    display: flex;
    height: 4px;
    pointer-events: none;
  }

  .progress.rtl {
    flex-direction: row-reverse;
  }

  .progress span {
    background: var(--spot);
  }

  .hud,
  .flash {
    position: absolute;
    left: 50%;
    z-index: 1;
    padding: 2px 10px;
    background: rgb(17 17 17 / 0.72);
    color: #f1f1ec;
    font-weight: 900;
    font-variant-numeric: tabular-nums;
    white-space: nowrap;
    pointer-events: none;
    transform: translateX(-50%);
  }

  .hud {
    bottom: calc(8px + env(safe-area-inset-bottom));
    font-size: 12px;
  }

  .flash {
    top: 16px;
    padding: 8px 16px;
    font-size: 14px;
  }

  .bar {
    position: absolute;
    right: 0;
    left: 0;
    z-index: 2;
    display: flex;
    gap: 12px;
    padding: 12px max(12px, env(safe-area-inset-right)) 12px max(12px, env(safe-area-inset-left));
    border: 0 solid var(--ink);
    background: var(--paper);
    color: var(--ink);
  }

  .top {
    top: 0;
    align-items: center;
    border-bottom-width: 3px;
  }

  .bottom {
    bottom: 0;
    flex-direction: column;
    padding-bottom: calc(12px + env(safe-area-inset-bottom));
    border-top-width: 3px;
  }

  .what {
    display: flex;
    flex: 1;
    flex-direction: column;
    min-width: 0;
  }

  .what strong,
  .what span {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .what strong {
    font-weight: 900;
  }

  .what span {
    font-size: 13px;
  }

  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 10px;
  }

  .line input {
    flex: 1 1 160px;
    min-width: 0;
    height: 44px;
    accent-color: var(--ink);
  }

  .line select {
    flex: 1 1 140px;
    min-width: 0;
  }

  .count {
    min-width: 72px;
    font-weight: 900;
    text-align: center;
  }

  /* ---------- the settings: a sheet beside the page, under it on a narrow screen ---------- */

  .sheet {
    position: absolute;
    top: 0;
    right: 0;
    bottom: 0;
    z-index: 3;
    display: flex;
    flex-direction: column;
    width: min(420px, 100%);
    border-left: 3px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
  }

  .sheet header {
    display: flex;
    flex: none;
    align-items: center;
    justify-content: space-between;
    padding: 10px 12px 10px 16px;
    border-bottom: 3px solid var(--ink);
    font-size: 20px;
  }

  .scroll {
    display: flex;
    flex-direction: column;
    gap: 18px;
    padding: 16px 20px calc(24px + env(safe-area-inset-bottom)) 16px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }

  .pick {
    display: flex;
    flex-direction: column;
    gap: 10px;
  }

  .pick label {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-weight: 900;
  }

  .pick p {
    color: var(--muted);
    font-size: 13px;
  }

  .seg {
    display: flex;
    border: 2px solid var(--ink);
  }

  .seg button {
    flex: 1;
    min-height: 40px;
    padding: 0 12px;
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

  @media (max-width: 700px) {
    .sheet {
      top: auto;
      left: 0;
      width: auto;
      max-height: 62dvh;
      border-top: 3px solid var(--ink);
      border-left: 0;
    }
  }
</style>
