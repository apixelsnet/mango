// How Mango is laid out on this device: its size, its covers, where its menu sits.
// Kept in this browser alone, unlike the theme: a phone and a desk want different answers.
// See docs/features/14-mobile.md.

export type Display = {
  /** Text and controls together, as a share of the size they are drawn at. */
  size: number;
  covers: 'small' | 'medium' | 'large';
  /** The menu as tabs along the bottom: on a narrow screen, always, or never. */
  tabs: 'auto' | 'on' | 'off';
  /** Nothing moves or fades: for an e-ink screen, which shows each frame of a movement. */
  still: boolean;
  /** The library a page at a time, this many titles to a page; 0 shows it whole. */
  pages: number;
  /** Reading here leaves no trace: no progress, no history, nothing sent to trackers. */
  incognito: boolean;
  /** What this device is, for the reader settings kept apart by device; `auto` tells by the screen. */
  device: 'auto' | Device;
};

export type Device = 'phone' | 'tablet' | 'computer';

export const DEVICES: [Device, string][] = [
  ['phone', 'Phone'],
  ['tablet', 'Tablet'],
  ['computer', 'Computer'],
];

export const SIZES: [number, string][] = [
  [0.9, 'Small'],
  [1, 'Normal'],
  [1.1, 'Large'],
  [1.25, 'Largest'],
];
export const COVERS: [Display['covers'], string][] = [
  ['small', 'Small'],
  ['medium', 'Medium'],
  ['large', 'Large'],
];
export const MENUS: [Display['tabs'], string][] = [
  ['auto', 'Automatic'],
  ['on', 'Bottom'],
  ['off', 'Top'],
];

export const PAGES: [number, string][] = [
  [0, 'All at once'],
  [24, '24 a page'],
  [48, '48 a page'],
  [96, '96 a page'],
];

export const DEFAULT_DISPLAY: Display = { size: 1, covers: 'medium', tabs: 'auto', still: false, pages: 0, incognito: false, device: 'auto' };

const KEY = 'mango.display';
/** Narrower than this, the menu moves to the bottom by itself. */
const NARROW = 760;

/** Fills in what is missing or broken, as for the theme. */
function sound(raw: unknown): Display {
  const d = { ...DEFAULT_DISPLAY, ...(raw && typeof raw === 'object' ? (raw as Partial<Display>) : {}) };
  return {
    size: SIZES.some(([size]) => size === d.size) ? d.size : DEFAULT_DISPLAY.size,
    covers: COVERS.some(([covers]) => covers === d.covers) ? d.covers : DEFAULT_DISPLAY.covers,
    tabs: MENUS.some(([tabs]) => tabs === d.tabs) ? d.tabs : DEFAULT_DISPLAY.tabs,
    still: d.still === true,
    pages: PAGES.some(([pages]) => pages === d.pages) ? d.pages : DEFAULT_DISPLAY.pages,
    incognito: d.incognito === true,
    device: DEVICES.some(([device]) => device === d.device) ? d.device : 'auto',
  };
}

/**
 * What the screen says this device is. A touch screen whose short side is under 600 CSS
 * pixels is a phone, a larger one a tablet; a mouse means a computer, touch screen or not.
 * The screen, not the window: a narrow window on a desk is still a computer.
 */
export function detectDevice(): Device {
  const touch = matchMedia('(pointer: coarse)').matches;
  if (!touch) return 'computer';
  return Math.min(screen.width, screen.height) < 600 ? 'phone' : 'tablet';
}

/** What this device is: as it was told in Settings, or as the screen says. */
export function thisDevice(): Device {
  return display.value.device === 'auto' ? detectDevice() : display.value.device;
}

function stored(): Display {
  try {
    return sound(JSON.parse(localStorage.getItem(KEY) ?? 'null'));
  } catch {
    return DEFAULT_DISPLAY;
  }
}

export const display = $state<{ value: Display; /** Whether the menu is along the bottom right now. */ tabs: boolean }>({
  value: stored(),
  tabs: false,
});

function apply() {
  const d = display.value;
  const root = document.documentElement;
  // A larger size leaves less room, as a narrower screen would.
  display.tabs = d.tabs === 'on' || (d.tabs === 'auto' && innerWidth / d.size <= NARROW);
  root.style.setProperty('--zoom', String(d.size));
  root.dataset.covers = d.covers;
  root.dataset.nav = display.tabs ? 'tabs' : 'top';
  root.dataset.still = d.still ? 'on' : 'off';
}

export function initDisplay() {
  apply();
  addEventListener('resize', apply);
}

export function setDisplay(patch: Partial<Display>) {
  display.value = sound({ ...display.value, ...patch });
  try {
    localStorage.setItem(KEY, JSON.stringify(display.value));
  } catch {
    // Private windows may refuse storage; the choice then lasts as long as the page.
  }
  apply();
}
