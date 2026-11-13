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

// ---------- continuous chapters: chapters 21 to 23 of the mock are a manhwa run ----------

/** The progress writes the reader sent, by chapter. */
const progress = async (page: Page) =>
  (await writes(page)).filter((write) => /\/progress$/.test(write.path)).map((write) => ({ chapter: Number(write.path.split('/')[3]), ...(write.body as object) }));

/** Marks the reader's node, so a test can tell whether it was made anew. */
const tag = (page: Page) => page.locator('.reader').evaluate((node) => ((node as HTMLElement & { tagged?: boolean }).tagged = true));
const tagged = (page: Page) => page.locator('.reader').evaluate((node) => !!(node as HTMLElement & { tagged?: boolean }).tagged);

test('a long strip runs on into the next chapter without opening it again', async ({ page }) => {
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  await tag(page);
  const strip = page.locator('.strip');

  // Scrolled down a screen at a time, as a reader would, until the next chapter's first page is up.
  const second = page.locator('.frame[data-page="6"]');
  for (let step = 0; step < 40 && !(await page.url()).includes('/read/22'); step++) {
    await strip.evaluate((box) => box.scrollBy({ top: box.clientHeight * 0.8 }));
    await page.waitForTimeout(120);
  }
  await expect(page.getByRole('separator', { name: 'Chapter 1 ends, Chapter 2 begins' })).toBeVisible();
  await expect(second).toBeAttached();
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect(page).toHaveTitle('Chapter 2 · Test manhwa · Mango');
  expect(await tagged(page)).toBe(true);
  // The chapter read to its end and left is kept as finished, once.
  await expect.poll(async () => (await progress(page)).filter((write) => write.chapter === 21 && 'read' in write)).toEqual([{ chapter: 21, page: 5, read: true }]);
});

test('a chapter dragged through is not kept as finished; one read on from is', async ({ page }) => {
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  const strip = page.locator('.strip');
  /** Puts a page of the run across the middle of the screen. */
  const center = (i: number) =>
    strip.evaluate((box, i) => {
      const frame = box.querySelector<HTMLElement>(`[data-page="${i}"]`)!;
      box.scrollTop = frame.offsetTop + frame.offsetHeight / 2 - box.clientHeight / 2;
    }, i);
  // Page 4 of chapter 1 brings chapter 2.
  await center(3);
  await expect(page.getByRole('separator')).toHaveCount(1);
  // Chapter 1's last page passes by, and the strip lands on page 4 of chapter 2.
  await center(5);
  await page.waitForTimeout(100);
  await center(9);
  await expect.poll(async () => (await progress(page)).filter((write) => write.chapter === 21).at(-1)).toEqual({ chapter: 21, page: 5 });
  // Back up in small steps to chapter 1, then read on into chapter 2: it is finished.
  const strip2 = page.locator('.strip');
  for (let step = 0; step < 40 && !/#\/read\/21$/.test(page.url()); step++) {
    await strip2.evaluate((box) => box.scrollBy({ top: -box.clientHeight * 0.5 }));
    await page.waitForTimeout(50);
  }
  await page.waitForTimeout(450);
  for (let step = 0; step < 40 && !page.url().includes('/read/22'); step++) {
    await strip2.evaluate((box) => box.scrollBy({ top: box.clientHeight * 0.5 }));
    await page.waitForTimeout(50);
  }
  await expect.poll(async () => (await progress(page)).filter((write) => write.chapter === 21).at(-1)).toEqual({ chapter: 21, page: 5, read: true });
});

test('turning past the last page opens the next chapter in place', async ({ page }) => {
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  await tag(page);
  await page.keyboard.press('m');
  await page.getByRole('combobox', { name: 'Reading direction' }).selectOption('ltr');
  await page.keyboard.press('Escape');
  for (let turn = 0; turn < 6; turn++) await page.keyboard.press('ArrowRight');
  await expect(page.getByRole('img', { name: 'Page 1 of 5' })).toBeVisible();
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  expect(await tagged(page)).toBe(true);
  // And back again, onto the last page of the chapter before.
  await page.keyboard.press('ArrowLeft');
  await expect(page.getByRole('img', { name: 'Page 6 of 6' })).toBeVisible();
  await expect(page).toHaveURL(/#\/read\/21$/);
});

test('with continuous chapters off, a strip ends on the Next chapter button', async ({ page }) => {
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  await page.keyboard.press('s');
  await page.getByRole('dialog', { name: 'Reader settings' }).getByRole('switch', { name: 'Continuous chapters' }).click();
  await page.keyboard.press('Escape');
  await page.locator('.strip').evaluate((box) => (box.scrollTop = box.scrollHeight));
  await expect(page.getByRole('button', { name: 'Next chapter' })).toBeVisible();
  await expect(page.getByRole('separator')).toHaveCount(0);
});

test('N in a strip goes to the next chapter in place, and a reload stays there', async ({ page }) => {
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  await tag(page);
  // Before the next chapter was added: it is fetched, then shown.
  await page.keyboard.press('n');
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect(page).toHaveTitle('Chapter 2 · Test manhwa · Mango');
  expect(await tagged(page)).toBe(true);
  // The chapter opened on, picked again from the list, is shown in place too.
  await page.keyboard.press('m');
  await page.getByRole('combobox', { name: 'Chapter' }).selectOption({ label: 'Chapter 1' });
  await expect(page).toHaveURL(/#\/read\/21$/);
  expect(await tagged(page)).toBe(true);
  await page.keyboard.press('Escape');
  await page.keyboard.press('n');
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await page.reload();
  await expect(page).toHaveTitle('Chapter 2 · Test manhwa · Mango');
});
