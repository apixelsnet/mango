// The theme: three colours picked by the owner, the rest derived.
// See docs/features/07-themes.md.

import { prefs, setPref } from './prefs.svelte';

export type Palette = { paper: string; ink: string; surface: string };

export type Theme = {
  /** Which of the two palettes shows: the system's choice, or one fixed. */
  follow: 'system' | 'light' | 'dark';
  /** A preset name, or "Custom". */
  light: string;
  dark: string;
  spot: string;
  custom: { paper: string; ink: string };
  shadows: boolean;
  dots: boolean;
};

export const PRESETS: Record<string, Palette> = {
  Paper: { paper: '#F1F1EC', ink: '#111111', surface: '#FFFFFF' },
  Newsprint: { paper: '#E2E1DA', ink: '#1A1A18', surface: '#F2F1EA' },
  Sepia: { paper: '#EDE1C9', ink: '#2A2017', surface: '#F7EEDB' },
  Midnight: { paper: '#151513', ink: '#F1F1EC', surface: '#232320' },
  'Ink blue': { paper: '#0F1A2B', ink: '#E9EEF7', surface: '#18263B' },
  'OLED black': { paper: '#000000', ink: '#F5F5F0', surface: '#141414' },
};

export const SPOTS: Record<string, string> = {
  Mango: '#FFC400',
  Tangerine: '#FF8A1F',
  Guava: '#FF6B8B',
  Mint: '#6EE7B7',
  Sky: '#6CC4FF',
  Lilac: '#B9A2FF',
};

export const DEFAULT_THEME: Theme = {
  follow: 'system',
  light: 'Paper',
  dark: 'Midnight',
  spot: '#FFC400',
  custom: { paper: '#F1F1EC', ink: '#111111' },
  shadows: true,
  dots: true,
};

const KEY = 'mango.theme';
const system = matchMedia('(prefers-color-scheme: dark)');

export const isHex = (value: unknown): value is string => typeof value === 'string' && /^#[0-9a-fA-F]{6}$/.test(value);

function channels(hex: string): number[] {
  const n = parseInt(hex.slice(1), 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function luminance(hex: string): number {
  const [r, g, b] = channels(hex).map((c) => {
    const v = c / 255;
    return v <= 0.03928 ? v / 12.92 : ((v + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

/** WCAG contrast ratio, 1 to 21. */
export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

function mix(a: string, b: string, share: number): string {
  const [from, to] = [channels(a), channels(b)];
  return '#' + from.map((c, i) => Math.round(c + (to[i] - c) * share).toString(16).padStart(2, '0')).join('');
}

/** Fills in what is missing or broken, so a half-saved or hand-edited theme still works. */
export function sound(raw: unknown): Theme {
  const t = { ...DEFAULT_THEME, ...(raw && typeof raw === 'object' ? (raw as Partial<Theme>) : {}) };
  const known = (name: unknown) => name === 'Custom' || (typeof name === 'string' && name in PRESETS);
  return {
    follow: t.follow === 'light' || t.follow === 'dark' ? t.follow : 'system',
    light: known(t.light) ? t.light : DEFAULT_THEME.light,
    dark: known(t.dark) ? t.dark : DEFAULT_THEME.dark,
    spot: isHex(t.spot) ? t.spot : DEFAULT_THEME.spot,
    custom: isHex(t.custom?.paper) && isHex(t.custom?.ink) ? { paper: t.custom.paper, ink: t.custom.ink } : DEFAULT_THEME.custom,
    shadows: t.shadows !== false,
    dots: t.dots !== false,
  };
}

export function palette(theme: Theme, name: string): Palette {
  if (name !== 'Custom') return PRESETS[name] ?? PRESETS.Paper;
  const { paper, ink } = theme.custom;
  const light = luminance(paper) > 0.4;
  return { paper, ink, surface: light ? mix(paper, '#FFFFFF', 0.6) : mix(paper, ink, 0.08) };
}

function stored(): Theme {
  try {
    return sound(JSON.parse(localStorage.getItem(KEY) ?? 'null'));
  } catch {
    return DEFAULT_THEME;
  }
}

export const theme = $state<{ value: Theme }>({ value: stored() });

/** The palette name in use right now. */
export function activeName(t: Theme = theme.value): string {
  const dark = t.follow === 'dark' || (t.follow === 'system' && system.matches);
  return dark ? t.dark : t.light;
}

export function apply(t: Theme = theme.value) {
  const colours = palette(t, activeName(t));
  const root = document.documentElement;
  root.style.setProperty('--paper', colours.paper);
  root.style.setProperty('--ink', colours.ink);
  root.style.setProperty('--surface', colours.surface);
  root.style.setProperty('--spot', t.spot);
  // Text on the spot colour is whichever of dark and light reads better.
  root.style.setProperty('--on-spot', contrast(t.spot, '#111111') >= contrast(t.spot, '#FFFFFF') ? '#111111' : '#FFFFFF');
  // A phone draws its own clock in white over an app opened from its home screen: the band behind it is the darker of the two.
  root.style.setProperty('--bar', luminance(colours.paper) < luminance(colours.ink) ? colours.paper : colours.ink);
  for (const meta of document.querySelectorAll('meta[name="theme-color"]')) meta.setAttribute('content', colours.paper);
  root.style.colorScheme = luminance(colours.paper) < 0.4 ? 'dark' : 'light';
  root.dataset.themed = 'yes';
  root.dataset.shadows = t.shadows ? 'on' : 'off';
  root.dataset.dots = t.dots ? 'on' : 'off';
}

function remember() {
  try {
    localStorage.setItem(KEY, JSON.stringify(theme.value));
  } catch {
    // Private windows may refuse storage; the server copy still holds.
  }
}

export function initTheme() {
  apply();
  system.addEventListener('change', () => apply());
}

/** The server's copy wins once it arrives: it is the one every device shares. */
export function adoptServerTheme() {
  if (prefs.values.theme === undefined) return;
  theme.value = sound(prefs.values.theme);
  remember();
  apply();
}

export async function saveTheme(next: Theme) {
  theme.value = sound(next);
  remember();
  apply();
  await setPref('theme', theme.value);
}
