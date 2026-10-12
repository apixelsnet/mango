// What a screen last showed, held for as long as the page lives, so that coming
// back to it shows it at once instead of an empty wait. Signing in or out starts
// the page over, which empties this: one account never sees what another's held.

const kept = new Map<string, unknown>();

export function recall<T>(key: string): T | null {
  return (kept.get(key) as T | undefined) ?? null;
}

export function keep(key: string, value: unknown) {
  kept.set(key, value);
}
