import { expect, test, type Page } from '@playwright/test';

// The main paths through the UI, against the stand-in API in mock.mjs: getting around,
// Undo, and the reader's loading, failed pages, keys and zoom.

const MOCK = 'http://127.0.0.1:8787';

type Write = { method: string; path: string; body: unknown };
const writes = async (page: Page): Promise<Write[]> => (await page.request.get(`${MOCK}/__writes`)).json();

test.beforeEach(async ({ page }) => {
  await page.request.post(`${MOCK}/__reset`);
});

test('each screen is named and takes the focus to its heading', async ({ page }) => {
  await page.goto('/#/');
  await expect(page).toHaveTitle('Mango');
  const library = page.getByRole('heading', { level: 1, name: 'Library' });
  await expect(library).toBeVisible();
  // Typed in normal case, shown in capitals.
  await expect(library).toHaveCSS('text-transform', 'uppercase');

  await page.getByRole('link', { name: 'History' }).first().click();
  await expect(page).toHaveTitle('History · Mango');
  await expect(page.getByRole('heading', { level: 1, name: 'History' })).toBeFocused();

  await page.getByRole('link', { name: 'Library' }).first().click();
  await page.getByRole('link', { name: 'Test title 1' }).first().click();
  await expect(page).toHaveTitle('Test title 1 · Mango');
  await expect(page.getByRole('heading', { level: 1, name: 'Test title 1' })).toBeFocused();
});

test('the first Tab reaches Skip to content', async ({ page }) => {
  await page.goto('/#/');
  await expect(page.getByRole('heading', { level: 1, name: 'Library' })).toBeVisible();
  await page.keyboard.press('Tab');
  await expect(page.getByRole('button', { name: 'Skip to content' })).toBeFocused();
});

test('Undo takes a change back before Mango hears of it', async ({ page }) => {
  const bar = page.locator('div.word');
  await page.goto('/#/title/1');
  await expect(page.getByRole('heading', { level: 1, name: 'Test title 1' })).toBeVisible();

  await page.getByRole('button', { name: 'More actions' }).click();
  await page.getByRole('button', { name: 'Mark all read' }).click();
  await expect(bar).toContainText('marked read');
  await bar.getByRole('button', { name: 'Undo' }).click();
  await expect(bar).toContainText('Undone.');
  expect(await writes(page)).toEqual([]);

  // Taken out of the library, then the screen is left: the change goes then.
  await page.getByRole('button', { name: 'In library' }).click();
  await expect(bar).toContainText('Taken out of the library.');
  expect(await writes(page)).toEqual([]);
  // On a phone a title hides the menu, so the address is changed as a link would.
  await page.evaluate(() => (location.hash = '#/history'));
  await expect.poll(() => writes(page)).toEqual([{ method: 'PUT', path: '/api/manga/1/library', body: { inLibrary: false } }]);
});

test('the reader names the chapter, says why a page failed and lists its keys', async ({ page }) => {
  await page.goto('/#/read/1');
  await expect(page).toHaveTitle('Chapter 1 · Test title 1 · Mango');
  await expect(page.getByRole('img', { name: 'Page 1 of 10' })).toBeVisible();

  // Manga reads right to left: the left arrow goes on. Page 3 always fails.
  await page.keyboard.press('ArrowLeft');
  await expect(page.getByRole('img', { name: 'Page 2 of 10' })).toBeVisible();
  await page.keyboard.press('ArrowLeft');
  // Tried again by itself after 1 s and 2 s, then the source's own words.
  await expect(page.getByText('The source answered 503 Service Unavailable.')).toBeVisible({ timeout: 15_000 });
  await expect(page.getByRole('button', { name: 'Try again' })).toBeVisible();

  await page.keyboard.press('?');
  const keys = page.getByRole('dialog', { name: /keys/i });
  await expect(keys).toBeVisible();
  await expect(keys).toBeFocused();
  await page.keyboard.press('Escape');
  await expect(keys).toBeHidden();
  await expect(page).toHaveURL(/#\/read\/1$/);
});

test('two fingers zoom the page, not the app, and turn nothing', async ({ page }) => {
  await page.goto('/#/read/1');
  await expect(page.getByRole('img', { name: 'Page 1 of 10' })).toBeVisible();
  const stage = page.locator('.stage');
  await expect(stage).toHaveCSS('touch-action', 'pan-x pan-y');
  const width = () => page.locator('.leaf').evaluate((leaf) => leaf.getBoundingClientRect().width);
  const before = await width();

  await stage.evaluate(async (node) => {
    const touch = (id: number, x: number) => new Touch({ identifier: id, target: node, clientX: x, clientY: 300 });
    const fire = (type: string, touches: Touch[], changed = touches) =>
      node.dispatchEvent(new TouchEvent(type, { touches, changedTouches: changed, bubbles: true, cancelable: true }));
    fire('touchstart', [touch(1, 150), touch(2, 200)]);
    for (let step = 1; step <= 5; step++) {
      fire('touchmove', [touch(1, 150 - step * 10), touch(2, 200 + step * 10)]);
      await new Promise((done) => setTimeout(done, 20));
    }
    fire('touchend', [], [touch(1, 100), touch(2, 250)]);
  });

  // The fingers went from 50 to 150 px apart: three times the size.
  await expect.poll(async () => (await width()) / before).toBeGreaterThan(2.8);
  await expect(page.getByRole('img', { name: 'Page 1 of 10' })).toBeVisible();
});
