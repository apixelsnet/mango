// What Settings › About shows of the releases: only what matters to this Mango, and
// only the part of the notes written for readers. Everything else is a link away.

import type { Release } from './api';

/** The most releases shown at once. */
export const SHOWN = 3;

/**
 * The releases worth showing, newest first: the ones newer than this Mango, at most
 * SHOWN of them, or else the newest. Pre-releases count only for someone on one.
 */
export function worthShowing(releases: Release[], current: string): Release[] {
  const onPre = current.includes('-');
  const kept = releases.filter((release) => onPre || !release.prerelease || release.version === current);
  const here = kept.findIndex((release) => release.version === current);
  const newer = here === -1 ? kept.slice(0, 1) : kept.slice(0, here);
  return (newer.length ? newer : kept.slice(0, 1)).slice(0, SHOWN);
}

/**
 * The "## Changes" part of a release's notes: what changed, in plain words. Empty for
 * notes without one, such as those of releases before 1.5.2.
 */
export function changes(notes: string): string {
  const lines = notes.replace(/\r\n?/g, '\n').split('\n');
  const start = lines.findIndex((line) => /^##\s+Changes\s*$/.test(line));
  if (start === -1) return '';
  const rest = lines.slice(start + 1);
  const end = rest.findIndex((line) => /^##\s/.test(line) || /^<!--/.test(line));
  return (end === -1 ? rest : rest.slice(0, end)).join('\n').trim();
}

/** The page listing every release, from one release's own page. */
export function allReleases(release: Release): string {
  return release.url.replace(/\/releases\/tag\/[^/]+\/?$/, '/releases');
}
