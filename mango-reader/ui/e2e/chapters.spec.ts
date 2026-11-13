import { expect, test, type Page } from '@playwright/test';

// Continuous chapters, against the stand-in API in mock.mjs: chapters 21 to 32 are a manhwa read
// one after another, six pages each but chapter 22 (five). Chapter N of the run is "Chapter N - 20".

const MOCK = 'http://127.0.0.1:8787';

type Write = { method: string; path: string; body: unknown };
const writes = async (page: Page): Promise<Write[]> => (await page.request.get(`${MOCK}/__writes`)).json();
/** The progress writes, by chapter. */
const progress = async (page: Page) =>
  (await writes(page)).filter((write) => /\/progress$/.test(write.path)).map((write) => ({ chapter: Number(write.path.split('/')[3]), ...(write.body as object) }));
/** The chapters written as finished, in order. */
const finished = async (page: Page) => (await progress(page)).filter((write) => 'read' in write).map((write) => write.chapter);

/** Marks the reader's node, so a test can tell whether it was made anew. */
const tag = (page: Page) => page.locator('.reader').evaluate((node) => ((node as HTMLElement & { tagged?: boolean }).tagged = true));
const tagged = (page: Page) => page.locator('.reader').evaluate((node) => !!(node as HTMLElement & { tagged?: boolean }).tagged);

/** Opens a chapter of the run and waits for its first page. */
async function open(page: Page, chapter = 21, query = '') {
  await page.goto(`/#/read/${chapter}${query}`);
  await expect(page.locator('.reader img').first()).toBeVisible();
}

/** Picks a reading direction from the controls. */
async function direction(page: Page, mode: string) {
  await page.keyboard.press('m');
  await page.getByRole('combobox', { name: 'Reading direction' }).selectOption(mode);
  await page.keyboard.press('Escape');
}

/** Flips a switch in the reader's settings sheet. */
async function toggle(page: Page, label: string) {
  await page.keyboard.press('s');
  await page.getByRole('dialog', { name: 'Reader settings' }).getByRole('switch', { name: label }).click();
  await page.keyboard.press('Escape');
}

/** Presses a key a number of times, as a reader would: a moment apart. */
async function press(page: Page, key: string, times: number) {
  for (let k = 0; k < times; k++) {
    await page.keyboard.press(key);
    await page.waitForTimeout(150);
  }
}

/** Puts the top of a page of the run a screen below the top of the strip: just before it comes up. */
const before = (page: Page, i: number) =>
  page.locator('.strip').evaluate((box, i) => {
    const frame = box.querySelector<HTMLElement>(`[data-page="${i}"]`)!;
    box.scrollTop = frame.offsetTop - box.clientHeight;
  }, i);

test.beforeEach(async ({ page }) => {
  await page.request.post(`${MOCK}/__reset`);
});

test('a strip reads through all twelve chapters to the last, each finished once', async ({ page }) => {
  test.setTimeout(120_000);
  await open(page);
  await tag(page);
  const strip = page.locator('.strip');
  const end = page.getByText('That was the last chapter.');
  // Long tasks while scrolling: a stall would show here.
  await page.evaluate(() => {
    const kept = ((window as unknown as { long: number[] }).long = []);
    new PerformanceObserver((list) => list.getEntries().forEach((entry) => kept.push(entry.duration))).observe({ type: 'longtask' });
  });
  // Down a screen at a time until the strip can go no further.
  const bottom = () => strip.evaluate((box) => box.scrollTop + box.clientHeight >= box.scrollHeight - 2);
  for (let step = 0; step < 400 && !(await bottom()); step++) {
    await strip.evaluate((box) => box.scrollBy({ top: box.clientHeight * 0.9 }));
    await page.waitForTimeout(40);
  }
  await expect(end).toBeInViewport();
  await expect(page).toHaveURL(/#\/read\/32(\?|$)/);
  await expect(page).toHaveTitle('Chapter 12 · Test manhwa · Mango');
  expect(await tagged(page)).toBe(true);
  await expect(page.getByRole('separator')).toHaveCount(11);
  await expect(page.locator('.frame')).toHaveCount(6 * 11 + 5);
  // Every chapter read on from is finished, once each, in order; the last one as its last page sat up.
  await expect.poll(() => finished(page)).toEqual(Array.from({ length: 12 }, (_, k) => 21 + k));
  // Only the pages near the screen are pictures in the document.
  expect(await page.locator('.frame img').count()).toBeLessThan(20);
  const long = await page.evaluate(() => (window as unknown as { long: number[] }).long);
  expect(Math.max(0, ...long)).toBeLessThan(250);
});

test('a fling across the end of a chapter still finishes it', async ({ page }) => {
  await open(page);
  const strip = page.locator('.strip');
  // Steps longer than a page, short of a jump: the page in the middle may skip the last one.
  for (let step = 0; step < 60 && !page.url().includes('/read/22'); step++) {
    await strip.evaluate((box) => box.scrollBy({ top: box.clientHeight * 1.4 }));
    await page.waitForTimeout(30);
  }
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect.poll(() => finished(page)).toEqual([21]);
});

test('auto-scroll runs on into the next chapter, and stops there when told not to carry on', async ({ page }) => {
  await open(page);
  const strip = page.locator('.strip');
  // Near the end of chapter 1, then auto-scroll at its fastest.
  await before(page, 5);
  for (let k = 0; k < 12; k++) await page.keyboard.press(']');
  await page.keyboard.press('a');
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/, { timeout: 15_000 });
  const top = await strip.evaluate((box) => box.scrollTop);
  await page.waitForTimeout(500);
  expect(await strip.evaluate((box) => box.scrollTop)).toBeGreaterThan(top);

  // Told not to carry on: it stops as chapter 3 comes up.
  await page.keyboard.press('a');
  await toggle(page, 'Carry on into the next chapter');
  await before(page, 10);
  await page.keyboard.press('a');
  await expect(page).toHaveURL(/#\/read\/23(\?|$)/, { timeout: 15_000 });
  await page.waitForTimeout(300);
  const stopped = await strip.evaluate((box) => box.scrollTop);
  await page.waitForTimeout(600);
  expect(await strip.evaluate((box) => box.scrollTop)).toBe(stopped);
});

test('paged auto-turning carries on into the next chapter in place', async ({ page }) => {
  await open(page, 21, '?page=4');
  await tag(page);
  await direction(page, 'ltr');
  await expect(page.getByRole('img', { name: 'Page 5 of 6' })).toBeVisible();
  // The whole page on the screen, so each tick turns; and a page every 2 s.
  await page.keyboard.press('f');
  for (let k = 0; k < 8; k++) await page.keyboard.press(']');
  await page.keyboard.press('a');
  await expect(page.getByRole('img', { name: 'Page 1 of 5' })).toBeVisible({ timeout: 10_000 });
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  expect(await tagged(page)).toBe(true);
  await expect.poll(() => finished(page)).toEqual([21]);
});

test('two pages side by side never pair the end of one chapter with the next', async ({ page }) => {
  await open(page);
  await direction(page, 'ltr');
  await page.keyboard.press('d');
  await expect(page.getByRole('img', { name: 'Page 2 of 6' })).toBeVisible();
  // Chapter 1: 1-2, 3-4, 5-6; chapter 2 (five pages): 1-2, 3-4, then 5 alone.
  await press(page, 'ArrowRight', 5);
  await expect(page.getByRole('img', { name: 'Page 5 of 5' })).toBeVisible();
  await expect(page.locator('.leaf img')).toHaveCount(1);
  await page.keyboard.press('ArrowRight');
  await expect(page).toHaveURL(/#\/read\/23(\?|$)/);
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  await expect(page.getByRole('img', { name: 'Page 2 of 6' })).toBeVisible();
});

test('right to left and vertical turn on into the next chapter, and back', async ({ page }) => {
  await open(page);
  await direction(page, 'rtl');
  await press(page, 'ArrowLeft', 6);
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect(page.getByRole('img', { name: 'Page 1 of 5' })).toBeVisible();
  await page.keyboard.press('ArrowRight');
  await expect(page).toHaveURL(/#\/read\/21$/);
  await expect(page.getByRole('img', { name: 'Page 6 of 6' })).toBeVisible();

  await direction(page, 'vertical');
  // A manhwa page fills the screen across, so Space scrolls down it first, then turns.
  for (let k = 0; k < 20 && !page.url().includes('/read/22'); k++) {
    await page.keyboard.press(' ');
    await page.waitForTimeout(300);
  }
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect(page.getByRole('img', { name: 'Page 1 of 5' })).toBeVisible();
});

test('the sideways strip runs across into the next chapter', async ({ page }) => {
  await open(page);
  await tag(page);
  await direction(page, 'sideways');
  const strip = page.locator('.strip');
  for (let step = 0; step < 60 && !page.url().includes('/read/22'); step++) {
    await strip.evaluate((box) => box.scrollBy({ left: box.clientWidth * 0.8 }));
    await page.waitForTimeout(60);
  }
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await expect(page.getByRole('separator', { name: 'Chapter 1 ends, Chapter 2 begins' })).toBeAttached();
  expect(await tagged(page)).toBe(true);
});

test('a next chapter that does not load says why, and comes on Try again', async ({ page }) => {
  await page.request.post(`${MOCK}/__fail`, { data: { chapter: 22 } });
  await open(page);
  await page.locator('.strip').evaluate((box) => (box.scrollTop = box.scrollHeight));
  await expect(page.getByText(/The next chapter did not load/)).toBeVisible();
  await expect(page.getByRole('button', { name: 'Next chapter' })).toBeVisible();
  // The source is back.
  await page.request.post(`${MOCK}/__reset`);
  await page.getByRole('button', { name: 'Try again' }).click();
  await expect(page.getByRole('separator')).toHaveCount(1);
  await expect(page.getByText(/The next chapter did not load/)).toBeHidden();
});

test('in a later chapter, the slider, page count, saved pages and P go by that chapter', async ({ page }) => {
  await open(page);
  await direction(page, 'ltr');
  await press(page, 'ArrowRight', 7);
  await expect(page.getByRole('img', { name: 'Page 2 of 5' })).toBeVisible();
  // B saves the page that is up, by its number in its own chapter.
  await page.keyboard.press('b');
  await expect
    .poll(async () => (await writes(page)).filter((write) => write.path.includes('/marks/')))
    .toEqual([{ method: 'PUT', path: '/api/chapters/22/marks/1', body: { note: '' } }]);
  await page.keyboard.press('m');
  await expect(page.locator('.count')).toHaveText('2 / 5');
  await expect(page.getByRole('slider', { name: 'Page' })).toHaveAttribute('aria-valuetext', 'Page 2 of 5');
  await page.getByRole('slider', { name: 'Page' }).fill('4');
  await expect(page.getByRole('img', { name: 'Page 5 of 5' })).toBeVisible();
  // Its last page sits up long enough to be kept as finished.
  await page.waitForTimeout(600);
  await page.keyboard.press('Escape');
  // P goes to the chapter before, in place, at its start.
  await page.keyboard.press('p');
  await expect(page).toHaveURL(/#\/read\/21$/);
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  // Chapter 1 was finished by turning on from it, chapter 2 by its last page sitting up; P finished nothing.
  await page.waitForTimeout(600);
  expect(await finished(page)).toEqual([21, 22]);
});

test('Back to the reader comes to the chapter it had reached', async ({ page }) => {
  await open(page);
  await page.keyboard.press('n');
  await expect(page).toHaveURL(/#\/read\/22(\?|$)/);
  await page.evaluate(() => (location.hash = '#/history'));
  await expect(page.getByRole('heading', { level: 1, name: 'History' })).toBeVisible();
  await page.goBack();
  await expect(page).toHaveTitle('Chapter 2 · Test manhwa · Mango');
  await expect(page.getByRole('img', { name: 'Page 1 of 5' }).first()).toBeVisible();
});

test('a press along the edge of the strip, where an overlay scrollbar is drawn, holds the next chapter back', async ({ page, isMobile }) => {
  test.skip(isMobile, 'A scrollbar is pressed with a mouse.');
  await open(page);
  const strip = page.locator('.strip');
  const edge = await strip.evaluate((box) => {
    const rect = box.getBoundingClientRect();
    return { x: rect.left + box.clientWidth - 5, y: rect.top + 20 };
  });
  // Firefox and Safari draw their scrollbar over the pages and give it no width: the press lands inside.
  await strip.dispatchEvent('pointerdown', { pointerType: 'mouse', clientX: edge.x, clientY: edge.y, buttons: 1, bubbles: true });
  for (let k = 1; k <= 20; k++) {
    await strip.evaluate((box, k) => (box.scrollTop = ((box.scrollHeight - box.clientHeight) * k) / 20), k);
    await page.waitForTimeout(30);
  }
  await page.waitForTimeout(800);
  await expect(page.getByRole('separator')).toHaveCount(0);
  await page.dispatchEvent('body', 'pointerup', { pointerType: 'mouse', bubbles: true });
  await expect(page.getByRole('separator')).toHaveCount(1);
});
