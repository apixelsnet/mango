// Settings › About shows only the releases that matter to this Mango, and only the part of
// their notes written for readers; the rest is a link away. The first tests run in Node,
// the last one in the browser against the stand-in API.

import { expect, test } from '@playwright/test';
import type { Release, VersionInfo } from '../src/lib/api';
import { allReleases, changes, SHOWN, worthShowing } from '../src/lib/releases';

const URL = 'https://github.com/example/mango/releases';
const NOTES = [
  '## Downloads',
  '',
  '| File | What it is |',
  '| --- | --- |',
  '| mango.zip | Windows |',
  '',
  '## Changes',
  '',
  '### Added',
  '- **Data saver**: smaller pages.',
  '',
  '### Fixed',
  '- A fix.',
  '',
  '<!-- Release notes generated using configuration in .github/release.yml -->',
  '',
  "## What's Changed",
  '* feat: something by @someone in https://github.com/example/mango/pull/1',
].join('\n');

const release = (version: string, notes = '', prerelease = false): Release => ({
  version,
  name: `Mango ${version}`,
  notes,
  url: `${URL}/tag/v${version}`,
  publishedAt: '2026-11-15T00:00:00Z',
  prerelease,
  download: null,
});

// Newest first, as the server sends them.
const LIST = ['1.6.0', '1.5.4', '1.5.3', '1.5.2', '1.5.1', '1.5.0'].map((v) => release(v));
const versions = (list: Release[]) => list.map((r) => r.version);

test.describe('which releases are shown', () => {
  test('on the newest, only that one', () => {
    expect(versions(worthShowing(LIST, '1.6.0'))).toEqual(['1.6.0']);
  });

  test('behind, the newer ones, at most a few', () => {
    expect(versions(worthShowing(LIST, '1.5.3'))).toEqual(['1.6.0', '1.5.4']);
    expect(versions(worthShowing(LIST, '1.5.0'))).toEqual(['1.6.0', '1.5.4', '1.5.3']);
    expect(worthShowing(LIST, '1.5.0')).toHaveLength(SHOWN);
  });

  test('a build the list does not know gets the newest', () => {
    expect(versions(worthShowing(LIST, '0.0.0-dev'))).toEqual(['1.6.0']);
  });

  test('pre-releases only for someone on one', () => {
    const list = [release('1.7.0-rc.1', '', true), ...LIST];
    expect(versions(worthShowing(list, '1.6.0'))).toEqual(['1.6.0']);
    expect(versions(worthShowing(list, '1.6.0-rc.2'))).toEqual(['1.7.0-rc.1']);
  });
});

test.describe('what of the notes is shown', () => {
  test('only the changes, without the downloads or the pull requests', () => {
    const text = changes(NOTES);
    expect(text.startsWith('### Added')).toBe(true);
    expect(text).toContain('**Data saver**');
    expect(text).toContain('- A fix.');
    expect(text).not.toMatch(/Downloads|What's Changed|pull\/1|<!--/);
  });

  test('notes from before the changelog have nothing to show', () => {
    expect(changes("## What's Changed\n* fix: a thing by @someone")).toBe('');
    expect(changes('')).toBe('');
  });

  test('every release is a link away', () => {
    expect(allReleases(release('1.5.2'))).toBe(URL);
  });
});

test('About shows the changes, folded after the first, and links to the rest', async ({ page }) => {
  const info: VersionInfo = {
    current: '1.5.2',
    commit: 'abc1234',
    platform: 'linux-x86_64',
    docker: true,
    canCheck: true,
    checkHours: 24,
    checkedAt: 1794700000,
    error: null,
    updateAvailable: true,
    latest: release('1.6.0', NOTES),
    releases: [release('1.6.0', NOTES), release('1.5.4', NOTES), release('1.5.3', "## What's Changed\n* old"), release('1.5.2', NOTES), release('1.5.1'), release('1.5.0')],
  };
  await page.route('**/api/version', (route) => route.fulfill({ json: info }));
  await page.goto('/#/settings/about');

  const card = page.locator('section', { has: page.getByRole('heading', { name: "What's new" }) });
  await expect(card).toBeVisible();
  await expect(card.locator('.releases > li')).toHaveCount(3);
  await expect(card.getByText('Mango 1.5.2')).toHaveCount(0);

  const first = card.locator('.releases > li').nth(0);
  await expect(first.locator('details')).toHaveAttribute('open', '');
  await expect(first.locator('.notes strong', { hasText: 'Data saver' })).toBeVisible();
  await expect(first).not.toContainText('Downloads');
  await expect(first).not.toContainText("What's Changed");

  const second = card.locator('.releases > li').nth(1);
  await expect(second.locator('details')).not.toHaveAttribute('open', '');
  await expect(second.locator('.notes')).toBeHidden();
  await second.getByText('What changed').click();
  await expect(second.locator('.notes')).toBeVisible();

  // Old notes: no changes to show, only the link to them.
  const third = card.locator('.releases > li').nth(2);
  await expect(third.locator('details')).toHaveCount(0);
  await expect(third.getByRole('link', { name: 'Full notes' })).toHaveAttribute('href', `${URL}/tag/v1.5.3`);

  await expect(card.getByRole('link', { name: 'Every release and its full notes' })).toHaveAttribute('href', URL);
});

test('on the newest release, its notes stay folded', async ({ page }) => {
  const newest = release('1.6.0', NOTES);
  const info: VersionInfo = {
    current: '1.6.0',
    commit: 'abc1234',
    platform: 'linux-x86_64',
    docker: true,
    canCheck: true,
    checkHours: 24,
    checkedAt: 1794700000,
    error: null,
    updateAvailable: false,
    latest: newest,
    releases: [newest, release('1.5.4', NOTES)],
  };
  await page.route('**/api/version', (route) => route.fulfill({ json: info }));
  await page.goto('/#/settings/about');

  const card = page.locator('section', { has: page.getByRole('heading', { name: "What's new" }) });
  await expect(card.locator('.releases > li')).toHaveCount(1);
  await expect(card.locator('details')).not.toHaveAttribute('open', '');
  await card.getByText('What changed').click();
  await expect(card.locator('.notes strong', { hasText: 'Data saver' })).toBeVisible();
});
