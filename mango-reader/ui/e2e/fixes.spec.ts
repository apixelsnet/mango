import { expect, test, type Page } from "@playwright/test";

// Fixes from the 2026-11-14 review, against the stand-in API in mock.mjs.

const MOCK = "http://127.0.0.1:8787";

type Write = { method: string; path: string; body: unknown };
const writes = async (page: Page): Promise<Write[]> =>
  (await page.request.get(`${MOCK}/__writes`)).json();
const progress = async (page: Page, chapter: number) =>
  (await writes(page)).filter(
    (w) => w.path === `/api/chapters/${chapter}/progress`,
  );

test.beforeEach(async ({ page }) => {
  await page.request.post(`${MOCK}/__reset`);
});

test("a chapter of one page is kept as finished when it is read", async ({
  page,
}) => {
  await page.goto("/#/read/40");
  await expect(page.locator("img").first()).toBeVisible();
  await expect
    .poll(() => progress(page, 40))
    .toEqual([
      {
        method: "PUT",
        path: "/api/chapters/40/progress",
        body: { page: 0, read: true },
      },
    ]);
});

test("a place Mango did not take in waits in the browser and goes once Mango answers", async ({
  page,
}) => {
  // The progress call fails as it does with no connection.
  await page.route("**/api/chapters/40/progress", (route) =>
    route.abort("internetdisconnected"),
  );
  await page.goto("/#/read/40");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          Object.keys(localStorage).filter((k) => k.startsWith("mango-unsent-"))
            .length,
      ),
    )
    .toBe(1);
  expect(await progress(page, 40)).toEqual([]);

  await page.unroute("**/api/chapters/40/progress");
  await page.evaluate(() => dispatchEvent(new Event("online")));
  await expect
    .poll(() => progress(page, 40))
    .toEqual([
      {
        method: "PUT",
        path: "/api/chapters/40/progress",
        body: { page: 0, read: true },
      },
    ]);
  expect(
    await page.evaluate(
      () =>
        Object.keys(localStorage).filter((k) => k.startsWith("mango-unsent-"))
          .length,
    ),
  ).toBe(0);
});

test("library bulk actions touch only the titles still shown", async ({
  page,
}) => {
  await page.goto("/#/");
  await expect(page.getByText("Test title 2").first()).toBeVisible();
  await page.getByRole("button", { name: "Select", exact: true }).click();
  await page.getByRole("button", { name: "All shown" }).click();
  await page.getByPlaceholder("Search the library").fill("Test title 2");
  await page.getByRole("button", { name: "Download unread" }).click();
  await expect
    .poll(
      async () =>
        (await writes(page)).find((w) => w.path === "/api/library/bulk")?.body,
    )
    .toEqual({ ids: [2], action: "download" });
});

test.describe(() => {
  // The service worker fetches the page's files itself, out of reach of page.route.
  test.use({ serviceWorkers: "block" });

  test("a screen whose file cannot be fetched reloads once, then says so", async ({
    page,
  }) => {
    await page.goto("/#/");
    await expect(page.getByText("Test title 2").first()).toBeVisible();
    // As after an upgrade: the screen's file is gone from the server.
    await page.route(/\/assets\/Stats-[^/]*\.js$/, (route) =>
      route.fulfill({ status: 404, body: "gone" }),
    );
    await page.evaluate(() => (location.hash = "#/stats"));
    await expect(
      page.getByText("This screen could not be loaded."),
    ).toBeVisible();
    await expect(page.getByRole("button", { name: "Try again" })).toBeVisible();
    expect(
      await page.evaluate(() =>
        sessionStorage.getItem("mango-reloaded-for-screen"),
      ),
    ).not.toBeNull();
  });
});
