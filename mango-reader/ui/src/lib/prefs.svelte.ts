// Preferences kept on the server, so they are the same on every device.

import { get, send } from './api';

export const prefs = $state<{ ready: boolean; values: Record<string, unknown> }>({ ready: false, values: {} });

export async function loadPrefs() {
  try {
    prefs.values = await get<Record<string, unknown>>('/settings');
  } catch {
    // Defaults will do until the server answers again.
  }
  prefs.ready = true;
}

export function pref<T>(key: string, fallback: T): T {
  const value = prefs.values[key];
  return value === undefined || value === null ? fallback : (value as T);
}

/** The languages Browse and Search keep to: English until others are picked, if a source has it. None means every language. */
export function browseLangs(sources: { lang: string }[]): string[] {
  return pref<string[] | null>('browse.langs', null) ?? (sources.some((s) => s.lang === 'en') ? ['en'] : []);
}

export async function setPref(key: string, value: unknown) {
  prefs.values[key] = value;
  await send('PUT', '/settings', { [key]: value });
}
