// How a chapter is shown, and where each answer comes from. Three layers, the later
// winning: what Mango ships for a kind of series, what Settings holds for that kind,
// and what the one title keeps for itself, set from inside the reader.

import { send } from './api';
import { pref, prefs } from './prefs.svelte';

export type Kind = 'manga' | 'manhwa' | 'manhua' | 'comic';
export type Mode = 'rtl' | 'ltr' | 'vertical' | 'webtoon' | 'sideways';

export type Prefs = {
  mode: Mode;
  fit: 'height' | 'width' | 'screen' | 'original';
  /** Two pages side by side: never, always, or when the screen is wider than tall. */
  spread: 'off' | 'on' | 'auto';
  /** The first page stands alone, which shifts every pair after it by one. */
  offset: boolean;
  /** Long strip: the widest a page gets, in pixels. */
  width: number;
  /** Long strip: the space between pages, in pixels. */
  gap: number;
  /** The plain margins round a scan are cut away. */
  crop: boolean;
  /** A page wider than tall is read as its two halves, one after the other. */
  split: boolean;
  /** How a page is scaled to the screen: smoothly, or keeping its edges hard. */
  scaling: 'smooth' | 'crisp' | 'pixelated';
  /** Sharpening, from none to strong. */
  sharpen: number;
  zones: 'sides' | 'kindle' | 'l' | 'off';
  flip: boolean;
  /** How far a tap or Space scrolls, in hundredths of the screen. */
  step: number;
  ahead: number;
  /** Auto-scroll: pixels a second down a strip, seconds a page otherwise. */
  autoSpeed: number;
  autoSeconds: number;
  autoNext: boolean;
  ground: 'black' | 'grey' | 'white' | 'theme';
  brightness: number;
  contrast: number;
  saturation: number;
  hue: number;
  sepia: number;
  grayscale: boolean;
  invert: boolean;
  tintAmount: number;
  tint: string;
  blend: 'normal' | 'multiply' | 'screen' | 'overlay' | 'darken' | 'lighten';
  number: boolean;
  bar: boolean;
  awake: boolean;
};

/** What one title keeps for itself: any of the settings, and the kind it was told it is. */
export type Own = Partial<Prefs> & { kind?: Kind };

export const KINDS: { id: Kind; name: string; all: string }[] = [
  { id: 'manga', name: 'Manga', all: 'All manga' },
  { id: 'manhwa', name: 'Manhwa', all: 'All manhwa' },
  { id: 'manhua', name: 'Manhua', all: 'All manhua' },
  { id: 'comic', name: 'Comic', all: 'All comics' },
];

const BASE: Prefs = {
  mode: 'rtl',
  fit: 'height',
  spread: 'off',
  offset: false,
  width: 900,
  gap: 0,
  crop: false,
  split: false,
  scaling: 'smooth',
  sharpen: 0,
  zones: 'sides',
  flip: false,
  step: 85,
  ahead: 3,
  autoSpeed: 80,
  autoSeconds: 8,
  autoNext: true,
  ground: 'black',
  brightness: 100,
  contrast: 100,
  saturation: 100,
  hue: 0,
  sepia: 0,
  grayscale: false,
  invert: false,
  tintAmount: 0,
  tint: '#ff9a3c',
  blend: 'multiply',
  number: false,
  bar: false,
  awake: true,
};

/** Where a kind of series parts from the rest as Mango ships it. */
const SHIPPED: Record<Kind, Partial<Prefs>> = {
  manga: {},
  manhwa: { mode: 'webtoon', fit: 'width', zones: 'off' },
  manhua: { mode: 'webtoon', fit: 'width', zones: 'off' },
  comic: { mode: 'ltr' },
};

// ---------- the settings, as the controls and the checks both read them ----------

export const MODES: [Mode, string][] = [
  ['rtl', 'Right to left'],
  ['ltr', 'Left to right'],
  ['vertical', 'Vertical'],
  ['webtoon', 'Long strip'],
  ['sideways', 'Sideways strip'],
];
export const FITS: [Prefs['fit'], string][] = [
  ['height', 'Fit height'],
  ['width', 'Fit width'],
  ['screen', 'Fit screen'],
  ['original', 'Original size'],
];

export type Field = { key: keyof Prefs; label: string; hint?: string; when?: (p: Prefs) => boolean } & (
  | { type: 'choice'; options: [string, string][] }
  | { type: 'range'; min: number; max: number; step: number; unit: string }
  | { type: 'switch' }
  | { type: 'colour' }
);

const paged = (p: Prefs) => p.mode !== 'webtoon' && p.mode !== 'sideways';
const strip = (p: Prefs) => p.mode === 'webtoon';
const continuous = (p: Prefs) => !paged(p);
const book = (p: Prefs) => p.mode === 'rtl' || p.mode === 'ltr';
const tinted = (p: Prefs) => p.tintAmount > 0;

export const GROUPS: { title: string; fields: Field[] }[] = [
  {
    title: 'Layout',
    fields: [
      { key: 'mode', label: 'Direction', type: 'choice', options: MODES },
      { key: 'fit', label: 'Page size', type: 'choice', options: FITS, when: paged },
      {
        key: 'spread',
        label: 'Two pages',
        hint: 'Side by side, like an open book. A page that is a spread already stays alone.',
        type: 'choice',
        options: [
          ['off', 'Never'],
          ['on', 'Always'],
          ['auto', 'On a wide screen'],
        ],
        when: book,
      },
      { key: 'offset', label: 'Cover on its own', hint: 'Shifts the pairs by one, for when spreads come out split down the middle.', type: 'switch', when: (p) => book(p) && p.spread !== 'off' },
      { key: 'width', label: 'Strip width', hint: 'The widest a page gets on a large screen.', type: 'range', min: 300, max: 3000, step: 50, unit: ' px', when: strip },
      { key: 'gap', label: 'Space between pages', type: 'range', min: 0, max: 100, step: 2, unit: ' px', when: continuous },
      { key: 'crop', label: 'Cut the margins', hint: 'The plain white or black border round a scan goes, so the art fills the screen.', type: 'switch' },
      { key: 'split', label: 'Split wide pages', hint: 'A spread is read as its two halves, one after the other, in the reading direction.', type: 'switch', when: paged },
    ],
  },
  {
    title: 'Auto-scroll',
    fields: [
      { key: 'autoSpeed', label: 'Speed', hint: 'Start and stop it with A; [ and ] change the speed.', type: 'range', min: 20, max: 600, step: 10, unit: ' px/s', when: continuous },
      { key: 'autoSeconds', label: 'Time on a page', hint: 'Start and stop it with A; [ and ] change the time.', type: 'range', min: 2, max: 60, step: 1, unit: ' s', when: paged },
      { key: 'autoNext', label: 'Carry on into the next chapter', type: 'switch' },
    ],
  },
  {
    title: 'Turning',
    fields: [
      {
        key: 'zones',
        label: 'Tap zones',
        hint: 'Sides: the edges turn, the middle shows the controls. Kindle: a narrow strip goes back. L: the top and left go back, the bottom and right go on.',
        type: 'choice',
        options: [
          ['sides', 'Sides'],
          ['kindle', 'Kindle'],
          ['l', 'L shape'],
          ['off', 'Off'],
        ],
      },
      { key: 'flip', label: 'Swap forward and back', type: 'switch', when: (p) => p.zones !== 'off' },
      { key: 'step', label: 'A tap or Space scrolls', hint: 'How much of the screen, where a page is taller than it.', type: 'range', min: 25, max: 100, step: 5, unit: '%' },
      { key: 'ahead', label: 'Pages loaded ahead', type: 'range', min: 1, max: 10, step: 1, unit: '', when: paged },
    ],
  },
  {
    title: 'Picture',
    fields: [
      {
        key: 'ground',
        label: 'Background',
        type: 'choice',
        options: [
          ['black', 'Black'],
          ['grey', 'Grey'],
          ['white', 'White'],
          ['theme', 'The theme’s paper'],
        ],
      },
      {
        key: 'scaling',
        label: 'Scaling',
        hint: 'Smooth suits most scans. Crisp keeps lines hard when a page is made bigger; Pixels keeps every dot square.',
        type: 'choice',
        options: [
          ['smooth', 'Smooth'],
          ['crisp', 'Crisp'],
          ['pixelated', 'Pixels'],
        ],
      },
      { key: 'sharpen', label: 'Sharpen', hint: 'Brings out the lines of a soft or blurry scan.', type: 'range', min: 0, max: 100, step: 10, unit: '%' },
      { key: 'brightness', label: 'Brightness', type: 'range', min: 30, max: 150, step: 5, unit: '%' },
      { key: 'contrast', label: 'Contrast', type: 'range', min: 50, max: 200, step: 5, unit: '%' },
      { key: 'saturation', label: 'Saturation', type: 'range', min: 0, max: 200, step: 5, unit: '%' },
      { key: 'hue', label: 'Hue', type: 'range', min: 0, max: 355, step: 5, unit: '°' },
      { key: 'sepia', label: 'Warmth', hint: 'A sepia wash, easier on the eyes at night.', type: 'range', min: 0, max: 100, step: 5, unit: '%' },
      { key: 'grayscale', label: 'Black and white', type: 'switch' },
      { key: 'invert', label: 'Invert', type: 'switch' },
      { key: 'tintAmount', label: 'Colour wash', hint: 'A colour laid over the page.', type: 'range', min: 0, max: 80, step: 5, unit: '%' },
      { key: 'tint', label: 'Wash colour', type: 'colour', when: tinted },
      {
        key: 'blend',
        label: 'Wash blends as',
        type: 'choice',
        options: [
          ['normal', 'Plain'],
          ['multiply', 'Multiply'],
          ['screen', 'Screen'],
          ['overlay', 'Overlay'],
          ['darken', 'Darken'],
          ['lighten', 'Lighten'],
        ],
        when: tinted,
      },
    ],
  },
  {
    title: 'On screen',
    fields: [
      { key: 'number', label: 'Page number', type: 'switch' },
      { key: 'bar', label: 'Progress bar', type: 'switch' },
      { key: 'awake', label: 'Keep the screen on', type: 'switch' },
    ],
  },
];

const FIELDS = GROUPS.flatMap((group) => group.fields);

/** The settings in `given` that are ones Mango knows, with values it can use. The rest is dropped. */
export function sane(given: unknown): Partial<Prefs> {
  const out: Record<string, unknown> = {};
  if (!given || typeof given !== 'object') return out;
  for (const field of FIELDS) {
    const value = (given as Record<string, unknown>)[field.key];
    const fits =
      field.type === 'choice'
        ? field.options.some(([option]) => option === value)
        : field.type === 'range'
          ? typeof value === 'number' && value >= field.min && value <= field.max
          : field.type === 'switch'
            ? typeof value === 'boolean'
            : typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value);
    if (fits) out[field.key] = value;
  }
  return out;
}

/** `base` with `patch` laid over it. A key the patch holds as undefined is let go of. */
export function merge<T extends object>(base: T, patch: Partial<T>): T {
  const out = { ...base, ...patch };
  for (const key of Object.keys(out) as (keyof T)[]) if (out[key] === undefined) delete out[key];
  return out;
}

// ---------- the layers ----------

type Profiles = Partial<Record<Kind, Partial<Prefs>>>;

/** How a kind of series is read when a title has nothing of its own to say. */
export function profile(kind: Kind): Prefs {
  // The one default Mango had before it told kinds apart still counts, for manga.
  const old = kind === 'manga' ? sane({ mode: pref<unknown>('reader.mode', undefined), fit: pref<unknown>('reader.fit', undefined) }) : {};
  return { ...BASE, ...SHIPPED[kind], ...old, ...sane(pref<Profiles>('reader.profiles', {})[kind]) };
}

let unsent: Record<string, unknown> = {};
let sending: ReturnType<typeof setTimeout> | undefined;

/** Changes what Settings holds for a kind. No patch puts the kind back to how Mango ships it. */
export function setProfile(kind: Kind, patch: Partial<Prefs> | null) {
  const all: Profiles = { ...pref<Profiles>('reader.profiles', {}) };
  if (patch) all[kind] = merge(all[kind] ?? {}, patch);
  else delete all[kind];
  const values: Record<string, unknown> = { 'reader.profiles': all };
  if (!patch && kind === 'manga') values['reader.mode'] = values['reader.fit'] = null;
  Object.assign(prefs.values, values);
  // A slider says a lot while it is dragged: write once it rests.
  Object.assign(unsent, values);
  clearTimeout(sending);
  sending = setTimeout(() => {
    send('PUT', '/settings', unsent).catch(() => {});
    unsent = {};
  }, 300);
}

/** What a title keeps for itself, out of what the server holds for it. */
export function owned(info: { readerMode: string | null; readerFit: string | null; readerPrefs: Record<string, unknown> | null }): Own {
  const raw = info.readerPrefs ?? {};
  const kind = KINDS.find((entry) => entry.id === raw.kind)?.id;
  return { ...sane({ ...raw, mode: info.readerMode ?? undefined, fit: info.readerFit ?? undefined }), ...(kind ? { kind } : {}) };
}

// ---------- what the settings come to ----------

/** The CSS filter the pages wear, or `none`. Sharpening is the SVG filter the reader draws, by its id. */
export function filterOf(p: Prefs): string {
  const parts = [
    p.sharpen > 0 && 'url(#mango-sharpen)',
    p.brightness !== 100 && `brightness(${p.brightness}%)`,
    p.contrast !== 100 && `contrast(${p.contrast}%)`,
    p.saturation !== 100 && `saturate(${p.saturation}%)`,
    p.hue !== 0 && `hue-rotate(${p.hue}deg)`,
    p.sepia !== 0 && `sepia(${p.sepia}%)`,
    p.grayscale && 'grayscale(1)',
    p.invert && 'invert(1)',
  ].filter(Boolean);
  return parts.join(' ') || 'none';
}

/** What a tap at `x`, `y` asks for, both from 0 to 1 across and down the page area. */
export function zone(p: Prefs, x: number, y: number): 'next' | 'prev' | 'menu' {
  let hit: 'next' | 'prev' | 'menu' = 'menu';
  if (p.zones === 'sides') {
    // Along the way the pages go: across for a book or a sideways strip, down for a column.
    const along = p.mode === 'vertical' || p.mode === 'webtoon' ? y : p.mode === 'rtl' ? 1 - x : x;
    hit = along > 0.7 ? 'next' : along < 0.3 ? 'prev' : 'menu';
  } else if (p.zones === 'kindle') {
    hit = y < 0.2 ? 'menu' : x < 0.3 ? 'prev' : 'next';
  } else if (p.zones === 'l') {
    hit = y < 0.25 || (x < 0.3 && y < 0.75) ? 'prev' : y > 0.75 || x > 0.7 ? 'next' : 'menu';
  }
  if (p.flip && hit !== 'menu') return hit === 'next' ? 'prev' : 'next';
  return hit;
}

/** The kernel of the sharpening filter: the centre pixel against its four neighbours. */
export function sharpenKernel(amount: number): string {
  const k = (amount / 100) * 1.2;
  return `0 ${-k} 0 ${-k} ${1 + 4 * k} ${-k} 0 ${-k} 0`;
}

// ---------- the keys ----------

export type Action =
  | 'next'
  | 'prev'
  | 'menu'
  | 'settings'
  | 'auto'
  | 'faster'
  | 'slower'
  | 'spread'
  | 'fit'
  | 'zoomIn'
  | 'zoomOut'
  | 'zoomReset'
  | 'mark'
  | 'nextChapter'
  | 'prevChapter';

/** What each key does unless the owner says otherwise. Arrows, Space, Home, End and Escape always do what they do. */
export const ACTIONS: { id: Action; label: string; keys: string[] }[] = [
  { id: 'next', label: 'Scroll on, then turn the page', keys: ['j', 'PageDown'] },
  { id: 'prev', label: 'Scroll back, then turn back', keys: ['k', 'PageUp'] },
  { id: 'nextChapter', label: 'Next chapter', keys: ['n'] },
  { id: 'prevChapter', label: 'Previous chapter', keys: ['p'] },
  { id: 'menu', label: 'Show or hide the controls', keys: ['m', 'Enter'] },
  { id: 'settings', label: 'Reader settings', keys: ['s'] },
  { id: 'auto', label: 'Auto-scroll on or off', keys: ['a'] },
  { id: 'faster', label: 'Auto-scroll faster', keys: [']'] },
  { id: 'slower', label: 'Auto-scroll slower', keys: ['['] },
  { id: 'spread', label: 'One page or two', keys: ['d'] },
  { id: 'fit', label: 'Next page size', keys: ['f'] },
  { id: 'zoomIn', label: 'Zoom in', keys: ['+', '='] },
  { id: 'zoomOut', label: 'Zoom out', keys: ['-'] },
  { id: 'zoomReset', label: 'Zoom back', keys: ['0'] },
  { id: 'mark', label: 'Save this page', keys: ['b'] },
];

/** Keys that keep their own work whatever is set: moving about, and leaving. */
export const FIXED = ['ArrowLeft', 'ArrowRight', 'ArrowUp', 'ArrowDown', ' ', 'Home', 'End', 'Escape', 'Tab'];

/** A key as it is stored and matched: a letter in lower case. */
export function keyName(key: string): string {
  return key.length === 1 ? key.toLowerCase() : key;
}

/** Each key to what it does, with the owner's own keys (`reader.keys`) in place of the shipped ones. */
export function keymap(): Map<string, Action> {
  const own = pref<Partial<Record<Action, string[]>>>('reader.keys', {});
  const map = new Map<string, Action>();
  for (const action of ACTIONS) for (const key of own[action.id] ?? action.keys) map.set(keyName(key), action.id);
  return map;
}
