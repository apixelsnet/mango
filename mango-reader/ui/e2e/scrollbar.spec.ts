import { expect, test } from '@playwright/test';

// Continuous chapters against a real scrollbar, which headless Chromium hides unless told not
// to: a press on it must hold the next chapter back until the mouse lets go. Chapters 21 to 23
// of the mock (mock.mjs) are a manhwa run.

const MOCK = 'http://127.0.0.1:8787';

test.use({ launchOptions: { ignoreDefaultArgs: ['--hide-scrollbars'] } });

test.beforeEach(async ({ page }) => {
  await page.request.post(`${MOCK}/__reset`);
});

test('a strip dragged by its scrollbar adds no chapter until let go, then one', async ({ page, isMobile, browserName }) => {
  test.skip(isMobile, 'A scrollbar is dragged with a mouse.');
  // Firefox and Safari draw no scrollbar beside the pages; chapters.spec.ts covers a press along the edge.
  test.skip(browserName !== 'chromium', 'Only Chromium gives the scrollbar its own room here.');
  await page.goto('/#/read/21');
  await expect(page.getByRole('img', { name: 'Page 1 of 6' })).toBeVisible();
  const strip = page.locator('.strip');
  const breaks = page.getByRole('separator');
  const box = await strip.evaluate((node) => {
    const rect = node.getBoundingClientRect();
    return { right: rect.right, top: rect.top, bottom: rect.bottom, bar: node.offsetWidth - node.clientWidth };
  });
  expect(box.bar).toBeGreaterThan(0);

  // The thumb pressed for real: the page hears the press and the release. Headless Chromium does
  // not move a thumb for a synthetic mouse, so the strip is scrolled to its end while it is held.
  const x = box.right - box.bar / 2;
  await page.mouse.move(x, box.top + 8);
  await page.mouse.down();
  for (let k = 1; k <= 20; k++) {
    await strip.evaluate((node, k) => (node.scrollTop = ((node.scrollHeight - node.clientHeight) * k) / 20), k);
    await page.waitForTimeout(30);
  }
  await page.waitForTimeout(1000);
  await expect(breaks).toHaveCount(0);
  await page.mouse.up();
  await expect(breaks).toHaveCount(1);
  await page.waitForTimeout(1000);
  await expect(breaks).toHaveCount(1);
});
