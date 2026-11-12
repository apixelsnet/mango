<script lang="ts" module>
  // Auto-scroll that ran off the end of a chapter carries on in the next one.
  let carry = false;
</script>

<script lang="ts">
  import { name } from '../lib/named.svelte';
  import { flushSync, untrack } from 'svelte';
  import { get, message, send, type PageMark, type ReaderInfo } from '../lib/api';
  import { DEVICES, display, thisDevice } from '../lib/display.svelte';
  import Icon from '../lib/Icon.svelte';
  import { makeOver, type Part } from '../lib/pages';
  import ReaderPrefs from '../lib/ReaderPrefs.svelte';
  import {
    ACTIONS,
    filterOf,
    FITS,
    keymap,
    keyName,
    KINDS,
    merge,
    MODES,
    byDevice,
    keep,
    owned,
    resolve,
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
  $effect(() => name(info && `${info.chapter.name} · ${info.title}`));
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
  // Pages whose picture has arrived, and those that have kept the reader waiting a while.
  let loaded = $state<Record<number, boolean>>({});
  let slow = $state<Record<number, boolean>>({});
  // A failed page is tried again by itself a couple of times before it asks: how often so far,
  // and whether it is waiting for its next try. Then why it failed, as Mango tells it.
  let again = $state<Record<number, number>>({});
  let waiting = $state<Record<number, boolean>>({});
  let why = $state<Record<number, string>>({});
  // The list of keys, over the page.
  let help = $state(false);
  // The page under the slider while it is being dragged.
  let peek = $state<number | null>(null);
  // What a screen reader is told: the page that is up, a page that failed.
  let spoken = $state('');
  // Webtoon only: which pages are close enough to be in the document, and their shapes once seen.
  let near = $state<Record<number, boolean>>({});
  let shapes = $state<Record<number, string>>({});
  // Pages wider than tall: a spread already, never paired with another.
  let wide = $state<Record<number, boolean>>({});
  let stage = $state<HTMLElement>();
  let strip = $state<HTMLElement>();
  // The pages of a strip, inside it: what auto-scroll shifts by the fraction of a pixel.
  let run = $state<HTMLElement>();
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
  const p = $derived<Prefs>(resolve(kind, own));
  // Page size and turning are kept by kind of device, unless Settings keeps one for all.
  const device = $derived(byDevice() ? DEVICES.find(([each]) => each === thisDevice())?.[1] : undefined);
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
      own = keep(own, Object.fromEntries(Object.keys(patch).map((key) => [key, undefined])));
    } else own = keep(own, patch);
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
      // The scanlation group reading followed into this chapter, handed on by the one before.
      const along = route.query.has('group') ? `?${new URLSearchParams({ group: route.query.get('group') ?? '' })}` : '';
      info = await get<ReaderInfo>(`/chapters/${id}${along}`);
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

  /**
   * Opens the chapter before or after this one, on its last page with `end`. The scanlation group
   * reading follows goes with it, so reading goes back to that group after another filled in for it.
   * A chapter picked from the list is opened without it, and its own group is followed from there.
   */
  function turnTo(chapter: number | null | undefined, end = false) {
    if (!chapter) return;
    const query = new URLSearchParams(end ? { at: 'end' } : {});
    if (typeof info?.group === 'string') query.set('group', info.group);
    const asked = String(query);
    go(`/read/${chapter}${asked ? `?${asked}` : ''}`, true);
  }

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
    else if (info?.next) turnTo(info.next);
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
    else if (info?.prev) turnTo(info.prev, true);
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
    if (sideways) box?.scrollBy({ left: ((box.clientWidth * p.step) / 100) * direction, behavior: glide() });
    else if (!paged || room('y', direction)) box?.scrollBy({ top: ((box.clientHeight * p.step) / 100) * direction, behavior: glide() });
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
        say('Page removed');
      } else {
        await send('PUT', `/chapters/${id}/marks/${page}`, { note: text });
        const kept = { chapterId: id, chapterName: info?.chapter.name ?? '', page, note: text.trim(), createdAt: Date.now() / 1000 };
        marks = [...marks.filter((mark) => mark.page !== page), kept];
        say('Page saved');
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
        image.onload = () => {
          seen(i, image);
          drawn(i);
        };
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
    const loader = new IntersectionObserver(
      (entries) =>
        entries.forEach((e) => {
          const i = page(e);
          near[i] = e.isIntersecting;
          // A page let go before it arrived starts its wait over when it comes back.
          if (!e.isIntersecting) since.delete(`${i}:${tries[i] ?? 0}`);
        }),
      {
      root: box,
        rootMargin: across ? '0px 250%' : '250% 0px',
      },
    );
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

  // ---------- loading, and pages that fail ----------

  /** How often a failed page is tried again before it asks, and how long a page loads before it is called slow. */
  const AUTO_TRIES = 2;
  const SLOW_MS = 10_000;

  /** A page's picture arrived. */
  function drawn(i: number) {
    loaded[i] = true;
    slow[i] = false;
  }

  /** A page did not load: tried again by itself after a moment, a couple of times, then it shows why and asks. */
  function failed(i: number) {
    if (waiting[i] || broken[i]) return;
    const done = again[i] ?? 0;
    if (done < AUTO_TRIES && navigator.onLine) {
      again[i] = done + 1;
      waiting[i] = true;
      setTimeout(() => {
        waiting[i] = false;
        loaded[i] = false;
        tries[i] = (tries[i] ?? 0) + 1;
      }, 1000 * 2 ** done);
      return;
    }
    broken[i] = true;
    slow[i] = false;
    speak(`Page ${i + 1} did not load.`);
    if (!navigator.onLine) {
      why[i] = 'This device is offline. The page loads again when it is back online.';
      return;
    }
    // The picture says nothing of why it failed; Mango's answer does.
    fetch(src(i))
      .then(async (response) => {
        if (response.ok) why[i] = 'The picture came but could not be shown.';
        else why[i] = (await response.json().catch(() => null))?.error?.message ?? `Mango answered ${response.status}.`;
      })
      .catch(() => (why[i] = 'Mango did not answer.'));
  }

  function retry(i: number) {
    tries[i] = (tries[i] ?? 0) + 1;
    broken[i] = false;
    loaded[i] = false;
    slow[i] = false;
    why[i] = '';
  }

  /** Every page that failed or is stuck, loaded again: R, or coming back online. */
  function retryAll() {
    const stuck = pages.filter((i) => broken[i] || slow[i]);
    stuck.forEach(retry);
    say(stuck.length ? `Loading ${stuck.length === 1 ? 'a page' : `${stuck.length} pages`} again` : 'No page to load again');
  }

  const failures = $derived(pages.filter((i) => broken[i]).length);

  // When each page on screen started loading, by page and try: past SLOW_MS it is slow, and offers a new try.
  const since = new Map<string, number>();
  $effect(() => {
    if (!ready) return;
    const on = paged ? shown : pages.filter((i) => near[i]);
    const late = on.filter((i) => !loaded[i] && !broken[i] && !waiting[i] && !slow[i]);
    if (!late.length) return;
    const now = Date.now();
    const due = late.map((i) => {
      const key = `${i}:${tries[i] ?? 0}`;
      if (!since.has(key)) since.set(key, now);
      return [i, since.get(key)! + SLOW_MS] as const;
    });
    const first = Math.min(...due.map(([, at]) => at));
    const timer = setTimeout(() => due.forEach(([i, at]) => at <= Date.now() && (slow[i] = true)), Math.max(0, first - now));
    return () => clearTimeout(timer);
  });

  // ---------- for screen readers and keyboards ----------

  let speaking: ReturnType<typeof setTimeout> | undefined;

  /** Tells a screen reader, once things have settled: turning fast through pages is not read out page by page. */
  function speak(text: string, wait = 0) {
    clearTimeout(speaking);
    speaking = setTimeout(() => (spoken = text), wait);
  }

  $effect(() => {
    if (!ready || !count) return;
    speak(shown.length > 1 ? `Pages ${shown[0] + 1} and ${reached + 1} of ${count}` : `Page ${reached + 1} of ${count}`, 700);
  });

  /** Scrolls glide unless the device asks for stillness. */
  const glide = (): ScrollBehavior => (display.value.still || matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth');

  /** Moves focus into a sheet or a dialog as it opens, and back where it was when it closes. */
  function focusIn(node: HTMLElement) {
    const before = document.activeElement;
    node.focus();
    // Tab goes round inside it, not out to the page behind, until it is closed.
    const round = (event: KeyboardEvent) => {
      if (event.key !== 'Tab') return;
      const stops = [...node.querySelectorAll<HTMLElement>('button, a[href], input, select, textarea, [tabindex]:not([tabindex="-1"])')].filter(
        (el) => !(el as HTMLButtonElement).disabled && el.offsetParent !== null,
      );
      if (!stops.length) return;
      const [first, last] = [stops[0], stops[stops.length - 1]];
      const at = document.activeElement;
      if (event.shiftKey && (at === first || at === node)) {
        event.preventDefault();
        last.focus();
      } else if (!event.shiftKey && at === last) {
        event.preventDefault();
        first.focus();
      }
    };
    node.addEventListener('keydown', round);
    return {
      destroy: () => {
        node.removeEventListener('keydown', round);
        if (before instanceof HTMLElement && before.isConnected) before.focus();
      },
    };
  }

  /** The keys as the reader goes by them, the owner's own included, for the list of keys. */
  const keyList = $derived(
    ACTIONS.map((action) => ({
      label: action.label,
      keys: [...keys].filter(([, what]) => what === action.id).map(([name]) => keyLabel(name)),
    })).filter((action) => action.keys.length),
  );

  function keyLabel(name: string): string {
    return name === ' ' ? 'Space' : name.length === 1 ? name.toUpperCase() : name;
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
      turnTo(info.next);
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
    const layer = run;
    if (!box || !layer) return;
    const pace = speed;
    // A sideways strip runs across, the long strip down.
    const across = sideways;
    const position = () => (across ? box.scrollLeft : box.scrollTop);
    const move = (to: number) => (across ? (box.scrollLeft = to) : (box.scrollTop = to));
    const atEnd = () => (across ? box.scrollLeft + box.clientWidth >= box.scrollWidth - 2 : box.scrollTop + box.clientHeight >= box.scrollHeight - 2);
    // A browser scrolls by whole pixels, of the page or of the screen, while a frame on a fast
    // screen moves the strip by less than one: it would stand still a frame, then jump. What
    // the scroll could not take is made up by shifting the pages, which moves by any fraction.
    const shift = (by: number) => (layer.style.transform = across ? `translate3d(${-by}px, 0, 0)` : `translate3d(0, ${-by}px, 0)`);
    layer.style.willChange = 'transform';
    shift(0);
    let at = position();
    let last = -1;
    let rested = 0;
    let frame = 0;
    const tick = (now: number) => {
      const passed = last < 0 ? 0 : Math.min(Math.max(now - last, 0), 100) / 1000;
      last = now;
      frame = requestAnimationFrame(tick);
      if (held) {
        shift(0);
        return;
      }
      // Scrolled by hand meanwhile: carry on from there.
      if (Math.abs(position() - at) > 2) at = position();
      at += pace * passed;
      move(at);
      // Short of where it should be, or past it: a browser may round either way.
      const short = at - position();
      shift(Math.abs(short) < 1 ? short : 0);
      if (!atEnd()) rested = 0;
      else if ((rested += passed) > 1.5) {
        rested = -Infinity;
        autoEnd();
      }
    };
    frame = requestAnimationFrame(tick);
    return () => {
      cancelAnimationFrame(frame);
      layer.style.transform = layer.style.willChange = '';
    };
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

  // ---------- handing a page on ----------

  // On a phone or tablet whose system has a share sheet that takes files, a page goes there;
  // elsewhere, a computer included, it is downloaded as a picture.
  const canShare = thisDevice() !== 'computer' && typeof navigator.canShare === 'function' && navigator.canShare({ files: [new File([''], 'page.jpg', { type: 'image/jpeg' })] });
  let handing = $state(false);

  async function handPage() {
    if (!info) return;
    const i = reached;
    handing = true;
    try {
      const answer = await fetch(src(i));
      if (!answer.ok) throw new Error(`Page ${i + 1} did not load.`);
      const blob = await answer.blob();
      const extension = (blob.type.split('/')[1] ?? 'jpg').replace('jpeg', 'jpg').replace(/\+.*/, '');
      const named = `${info.title} - ${info.chapter.name} - page ${i + 1}`.replace(/[\\/:*?"<>|]+/g, ' ').trim();
      const file = new File([blob], `${named}.${extension}`, { type: blob.type });
      if (canShare && navigator.canShare({ files: [file] })) {
        await navigator.share({ files: [file], title: named });
      } else {
        const url = URL.createObjectURL(file);
        const link = Object.assign(document.createElement('a'), { href: url, download: file.name });
        link.click();
        setTimeout(() => URL.revokeObjectURL(url), 10_000);
        say(`Page ${i + 1} downloaded`);
      }
    } catch (e) {
      // Closing the share sheet is not a failure.
      if (!(e instanceof DOMException && e.name === 'AbortError')) say(message(e));
    } finally {
      handing = false;
    }
  }

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

  // ---------- zoom ----------

  /**
   * Zooms to `to`, keeping the spot at `x`, `y` (from the box's corner, on screen) where it
   * is: under the fingers of a pinch, under the pointer of a wheel. Pages grow in step with
   * the zoom, so the scroll is scaled with it once the new size is laid out.
   */
  function zoomAt(to: number, x?: number, y?: number) {
    const box = paged ? stage : strip;
    if (!box || to === zoom) return;
    const ratio = to / zoom;
    const [left, top] = [box.scrollLeft, box.scrollTop];
    const [cx, cy] = [x ?? box.clientWidth / 2, y ?? box.clientHeight / 2];
    zoom = to;
    flushSync();
    box.scrollLeft = (left + cx) * ratio - cx;
    box.scrollTop = (top + cy) * ratio - cy;
  }

  // Two fingers on the page: how far apart they started, and the zoom then.
  let pinch: { gap: number; zoom: number } | null = null;
  const gapOf = (t: TouchList) => Math.hypot(t[0].clientX - t[1].clientX, t[0].clientY - t[1].clientY);

  function pinchStart(event: TouchEvent) {
    if (event.touches.length !== 2) return;
    pinch = { gap: gapOf(event.touches), zoom };
    // A pinch is not a swipe.
    touch = null;
  }

  function pinchMove(event: TouchEvent) {
    const box = paged ? stage : strip;
    if (!pinch || !box || event.touches.length !== 2 || pinch.gap < 1) return;
    const [a, b] = [event.touches[0], event.touches[1]];
    const rect = box.getBoundingClientRect();
    zoomAt(clamp((pinch.zoom * gapOf(event.touches)) / pinch.gap), (a.clientX + b.clientX) / 2 - rect.left, (a.clientY + b.clientY) / 2 - rect.top);
  }

  function pinchEnd(event: TouchEvent) {
    if (!pinch || event.touches.length >= 2) return;
    pinch = null;
    // Close enough to the page's own size is taken as it.
    if (Math.abs(zoom - 1) < 0.08) zoomAt(1);
    speak(`Zoom ${Math.round(zoom * 100)}%`);
  }

  function wheel(event: WheelEvent) {
    if (event.ctrlKey) {
      event.preventDefault();
      const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
      zoomAt(clamp(zoom * (event.deltaY < 0 ? 1.1 : 0.9)), event.clientX - rect.left, event.clientY - rect.top);
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
        turnTo(info?.next);
        break;
      case 'prevChapter':
        turnTo(info?.prev);
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
        zoomAt(clamp(zoom * 1.2));
        say(`Zoom ${Math.round(zoom * 100)}%`);
        break;
      case 'zoomOut':
        zoomAt(clamp(zoom / 1.2));
        say(`Zoom ${Math.round(zoom * 100)}%`);
        break;
      case 'zoomReset':
        zoomAt(1);
        say('Zoom 100%');
        break;
      case 'mark':
        keepPage(marked ? null : '');
        break;
      case 'retry':
        retryAll();
        break;
      case 'keys':
        help = true;
        overlay = panel = false;
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
      else if (help) help = false;
      else if (zones) closeZones();
      else if (panel) panel = false;
      else if (overlay) overlay = false;
      else if (info) up(`/title/${info.mangaId}`);
      event.preventDefault();
      return;
    }
    if (on?.matches('input, select, textarea')) return;
    // Over the list of keys, only its own key does anything; Tab still moves about in it.
    if (help) {
      if (keys.get(keyName(event.key)) === 'keys') {
        help = false;
        event.preventDefault();
      }
      return;
    }
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

<svelte:window
  onkeydown={key}
  onpagehide={() => save(true)}
  ononline={() => pages.filter((i) => broken[i]).forEach(retry)}
  bind:innerWidth={vw}
  bind:innerHeight={vh}
/>

{#snippet loading(i: number, live: boolean)}
  <div class="loading" class:late={slow[i]} role={live ? 'status' : undefined}>
    <span class="spinner" aria-hidden="true"></span>
    <span class="says">
      {waiting[i] ? `Trying page ${i + 1} again…` : slow[i] ? `Page ${i + 1} is slow to load` : `Loading page ${i + 1}…`}
    </span>
    {#if slow[i]}
      <button class="btn" onclick={() => retry(i)}><Icon name="refresh" size={16} />Try again</button>
    {/if}
  </div>
{/snippet}

{#snippet failure(i: number)}
  <div class="panel message failed">
    <strong>Page {i + 1} did not load.</strong>
    {#if why[i]}<span>{why[i]}</span>{/if}
    <div class="tools">
      <button class="btn ink" onclick={() => retry(i)}><Icon name="refresh" size={16} />Try again</button>
      {#if failures > 1}<button class="btn" onclick={retryAll}>Try all {failures} again</button>{/if}
    </div>
  </div>
{/snippet}
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
    <div class="center" role="status">
      <span class="spinner" aria-hidden="true"></span>
      <span>Opening…</span>
    </div>
  {:else if paged}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div
      class="stage"
      data-fit={fit}
      style:--z={zoom}
      bind:this={stage}
      onclick={tap}
      ontouchstart={(e) => (touchstart(e), pinchStart(e))}
      ontouchmove={pinchMove}
      ontouchend={(e) => (pinchEnd(e), touchend(e))}
      ontouchcancel={(e) => ((held = false), (touch = null), pinchEnd(e))}
      onwheel={wheel}
    >
      {#key shown.join()}
        <span class="number display" aria-hidden="true">{shown[0] + 1}</span>
        <div class="leaf" class:two={shown.length > 1} class:rtl={mode === 'rtl'}>
          {#each shown as i (`${i}:${tries[i] ?? 0}`)}
            {#if broken[i]}
              {@render failure(i)}
            {:else if !waiting[i]}
              <img
                src={look(i)}
                alt="Page {i + 1} of {count}{splitting(i) ? `, ${half === 0 ? 'first' : 'second'} half` : ''}"
                draggable="false"
                onload={(e) => {
                  if (look(i) === src(i)) seen(i, e.currentTarget as HTMLImageElement);
                  drawn(i);
                }}
                onerror={() => failed(i)}
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
      ontouchstart={(e) => ((held = true), pinchStart(e))}
      ontouchmove={pinchMove}
      ontouchend={(e) => ((held = e.touches.length > 0), pinchEnd(e))}
      ontouchcancel={(e) => ((held = false), pinchEnd(e))}
    >
      <div class="run" bind:this={run}>
        {#each pages as i (i)}
          <div
            class="frame"
            data-page={i}
            aria-busy={near[i] && !loaded[i] && !broken[i]}
            style:aspect-ratio={shapes[i] ?? '2 / 3'}
            style:width={sideways ? null : `${100 * zoom}%`}
            style:max-width={sideways ? null : `${p.width * zoom}px`}
            style:margin-bottom={sideways ? null : `${p.gap}px`}
            style:margin-right={sideways ? `${p.gap}px` : null}
          >
            {#if broken[i]}
              {@render failure(i)}
            {:else if near[i]}
              {#if !waiting[i]}
                <!-- Decoded off the main thread: a tall page arriving mid-scroll does not stall it. -->
                <img
                  src={look(i, 'whole')}
                  alt="Page {i + 1} of {count}"
                  draggable="false"
                  decoding="async"
                  onload={(e) => {
                    measured(i, e.currentTarget as HTMLImageElement);
                    drawn(i);
                  }}
                  onerror={() => failed(i)}
                />
              {/if}
              {#if !loaded[i]}{@render loading(i, false)}{/if}
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
    </div>
  {/if}

  {#if ready && !error}
    {#if paged}
      {@const busy = shown.filter((i) => !loaded[i] && !broken[i])}
      {#if busy.length}{@render loading(busy[0], true)}{/if}
    {/if}
    {#if p.tintAmount > 0}
      <div class="tint" style:background={p.tint} style:opacity={p.tintAmount / 100} style:mix-blend-mode={p.blend}></div>
    {/if}
    {#if p.bar}
      <div class="progress" class:rtl={mode === 'rtl'} aria-hidden="true"><span style:width="{((reached + 1) / count) * 100}%"></span></div>
    {/if}
    {#if p.number && !overlay && !panel}
      <span class="hud" aria-hidden="true">{shown.length > 1 ? `${shown[0] + 1}–${reached + 1}` : reached + 1} / {count}{marked ? ' · saved' : ''}</span>
    {/if}
    {#if noting !== null}
      <form class="note" onsubmit={(e) => (e.preventDefault(), keepPage(noting ?? ''))}>
        <label>
          <span>A note for page {reached + 1}</span>
          <!-- svelte-ignore a11y_autofocus -->
          <input bind:value={noting} maxlength="2000" placeholder="What is on this page" autofocus />
        </label>
        <div class="tools">
          <button class="btn ink">Save</button>
          {#if marked}<button class="btn" type="button" onclick={() => keepPage(null)}>Remove</button>{/if}
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
        aria-label={marked ? `Page ${reached + 1} is saved: change its note` : `Save page ${reached + 1} with a note`}
        aria-pressed={!!marked}
        onclick={() => ((noting = marked?.note ?? ''), (overlay = false))}
      >
        <Icon name="bookmark" fill={!!marked} />
      </button>
      {#if document.fullscreenEnabled}
        <button class="btn icon" aria-label="Full screen" aria-pressed={full} onclick={fullscreen}><Icon name="expand" /></button>
      {/if}
      {#if thisDevice() === 'computer'}
        <button class="btn icon" aria-label="Show the keys" onclick={() => ((help = true), (overlay = false))}><Icon name="info" /></button>
      {/if}
      {#if p.zones !== 'off'}
        <button class="btn icon" aria-label="Show the tap zones" onclick={() => ((zones = true), (overlay = false))}><Icon name="grid" /></button>
      {/if}
      <button class="btn icon" aria-label="Hide the controls" onclick={() => (overlay = false)}><Icon name="close" /></button>
    </div>
    <div class="bar bottom">
      {#if note}<p role="status">{note}</p>{/if}
      <div class="line">
        <button class="btn icon" aria-label="Previous chapter" disabled={!info.prev} onclick={() => turnTo(info?.prev)}>
          <Icon name="first" />
        </button>
        <div class="scrub">
          {#if peek !== null}
            <!-- While the slider is dragged, the page under it is shown small; the reader turns to it on letting go. -->
            <div class="peek" aria-hidden="true" style:--at={count > 1 ? peek / (count - 1) : 0} class:rtl={mode === 'rtl'}>
              <img src={src(peek)} alt="" draggable="false" />
              <strong>{peek + 1}</strong>
            </div>
          {/if}
          <input
            type="range"
            min="0"
            max={count - 1}
            value={index}
            dir={mode === 'rtl' ? 'rtl' : 'ltr'}
            aria-label="Page"
            aria-valuetext="Page {index + 1} of {count}"
            onpointerdown={(e) => (peek = Number(e.currentTarget.value))}
            oninput={(e) => {
              const to = Number(e.currentTarget.value);
              if (peek === null) show(to);
              else peek = to;
            }}
            onpointerup={(e) => {
              if (peek !== null) show(Number(e.currentTarget.value));
              peek = null;
            }}
            onpointercancel={() => (peek = null)}
          />
        </div>
        <strong class="count">{reached + 1} / {count}</strong>
        <button class="btn icon" aria-label="Next chapter" disabled={!info.next} onclick={() => turnTo(info?.next)}>
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
        <button class="btn icon" aria-label={canShare ? `Share page ${reached + 1}` : `Download page ${reached + 1}`} disabled={handing} onclick={handPage}>
          <Icon name={canShare ? 'share' : 'download'} />
        </button>
      </div>
    </div>
  {/if}

  {#if zones && info && ready}
    <Zones {p} onclose={closeZones} />
  {/if}

  {#if panel && info && ready}
    {@const entry = KINDS.find((each) => each.id === kind) ?? KINDS[0]}
    <div class="sheet" role="dialog" aria-modal="true" aria-label="Reader settings" tabindex="-1" use:focusIn>
      <header>
        <strong class="display caps">Reader</strong>
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
            {#if device}Settings marked {device} are kept for every {device.toLowerCase()} and no other device.{/if}
          </p>
        </div>
        <ReaderPrefs value={p} onchange={change} {device} />
        <button class="btn" disabled={!Object.keys(own).length} onclick={forget}>Forget what this title keeps for itself</button>
      </div>
    </div>
  {/if}

  {#if help}
    <div class="keys" role="dialog" aria-modal="true" aria-labelledby="keys-title" tabindex="-1" use:focusIn>
      <header>
        <strong id="keys-title" class="display">KEYS</strong>
        <button class="btn icon" aria-label="Close the keys" onclick={() => (help = false)}><Icon name="close" /></button>
      </header>
      <dl>
        <dt><kbd>←</kbd> <kbd>→</kbd></dt>
        <dd>Turn the page, the way the book reads</dd>
        <dt><kbd>↑</kbd> <kbd>↓</kbd></dt>
        <dd>Scroll</dd>
        <dt><kbd>Space</kbd> <kbd>Shift</kbd>+<kbd>Space</kbd></dt>
        <dd>Scroll on or back, then turn the page</dd>
        <dt><kbd>Home</kbd> <kbd>End</kbd></dt>
        <dd>First or last page</dd>
        <dt><kbd>Esc</kbd></dt>
        <dd>Close what is open, then leave the reader</dd>
        {#each keyList as action (action.label)}
          <dt>{#each action.keys as name, n (name)}{n ? ' ' : ''}<kbd>{name}</kbd>{/each}</dt>
          <dd>{action.label}</dd>
        {/each}
      </dl>
      <p>Change these in Settings › Reader.</p>
    </div>
  {/if}

  <div class="sr" aria-live="polite" aria-atomic="true">{spoken}</div>
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

  .reader[data-ground='gray'] {
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
    flex-direction: column;
    gap: 16px;
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
    /* Two fingers zoom the page, not the whole app around it. */
    touch-action: pan-x pan-y;
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

  /* The spinner names the page itself. */
  .reader:has(> .loading) .number {
    display: none;
  }

  .strip {
    height: 100%;
    overflow: auto;
    overscroll-behavior: contain;
    touch-action: pan-x pan-y;
  }

  .frame {
    position: relative;
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
    overflow-x: auto;
    overflow-y: hidden;
  }

  .sideways .run {
    display: flex;
    align-items: stretch;
    height: 100%;
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

  /* ---------- a page on its way, or one that failed ---------- */

  /* Over the screen in the paged modes, over its own frame in a strip. It shows only after a moment, so a quick page does not flicker. */
  .loading {
    position: absolute;
    inset: 0;
    z-index: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 14px;
    padding: 16px;
    pointer-events: none;
    animation: appear 0.2s 0.3s both;
  }

  .loading .btn {
    pointer-events: auto;
  }

  .says {
    padding: 4px 12px;
    background: rgb(17 17 17 / 0.78);
    color: #f1f1ec;
    font-size: 14px;
    font-weight: 900;
    text-align: center;
  }

  .spinner {
    flex: none;
    width: 44px;
    height: 44px;
    border: 5px solid rgb(128 128 128 / 0.35);
    border-top-color: var(--spot);
    border-radius: 50%;
    animation: spin 0.9s linear infinite;
  }

  /* Still a sign of work, without the turning. */
  @media (prefers-reduced-motion: reduce) {
    .spinner {
      border-color: var(--spot);
      animation: pulse 1.6s ease-in-out infinite alternate;
    }
  }

  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  @keyframes pulse {
    to {
      opacity: 0.3;
    }
  }

  @keyframes appear {
    from {
      opacity: 0;
    }
  }

  .failed {
    align-self: center;
    margin: auto;
  }

  .failed span {
    color: var(--muted);
    font-size: 14px;
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

  /* Windows' contrast themes drop background and border colors: the spinner's turning part and the progress keep the system's highlight. */
  @media (forced-colors: active) {
    .spinner {
      forced-color-adjust: none;
      border-color: GrayText;
      border-top-color: Highlight;
    }

    .progress span {
      forced-color-adjust: none;
      background: Highlight;
    }
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

  .scrub {
    position: relative;
    display: flex;
    flex: 1 1 160px;
    min-width: 0;
  }

  .scrub input {
    flex: 1;
    min-width: 0;
    height: 44px;
    accent-color: var(--ink);
  }

  /* Rides above the slider's thumb: --at is how far along, 0 to 1. */
  .peek {
    position: absolute;
    bottom: calc(100% + 10px);
    left: clamp(0px, calc(var(--at) * 100% - 48px), calc(100% - 96px));
    display: flex;
    flex-direction: column;
    align-items: center;
    width: 96px;
    padding: 4px;
    border: 3px solid var(--ink);
    background: var(--paper);
    box-shadow: 4px 4px 0 var(--ink);
    pointer-events: none;
  }

  .peek.rtl {
    left: clamp(0px, calc((1 - var(--at)) * 100% - 48px), calc(100% - 96px));
  }

  .peek img {
    width: 100%;
    height: 132px;
    object-fit: contain;
    background: #111111;
  }

  .peek strong {
    padding-top: 2px;
    font-weight: 900;
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

  .sheet:focus,
  .keys:focus {
    outline: none;
  }

  /* ---------- the list of keys ---------- */

  .keys {
    position: absolute;
    top: 50%;
    left: 50%;
    z-index: 4;
    display: flex;
    flex-direction: column;
    gap: 12px;
    width: min(520px, calc(100% - 32px));
    max-height: calc(100% - 32px);
    padding: 12px 16px 16px;
    overflow-y: auto;
    border: 3px solid var(--ink);
    background: var(--paper);
    color: var(--ink);
    box-shadow: var(--sh-5);
    transform: translate(-50%, -50%);
  }

  .keys header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 20px;
  }

  .keys dl {
    display: grid;
    grid-template-columns: auto 1fr;
    gap: 8px 16px;
    margin: 0;
  }

  .keys dt {
    white-space: nowrap;
  }

  .keys dd {
    margin: 0;
  }

  .keys kbd {
    display: inline-block;
    min-width: 26px;
    padding: 1px 6px;
    border: 2px solid var(--ink);
    background: var(--surface);
    font-family: inherit;
    font-size: 13px;
    font-weight: 700;
    text-align: center;
  }

  .keys p {
    color: var(--muted);
    font-size: 13px;
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
