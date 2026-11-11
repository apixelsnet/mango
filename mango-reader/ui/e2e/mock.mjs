// A stand-in for Mango's API, for the browser smoke test (smoke.spec.ts). It answers only
// what the screens under test ask, from made-up data, on the port `npm run dev` and
// `vite preview` send /api to (vite.config.ts).
//
// Pages of a chapter: page 3 (index 2) always fails with the source's message, page 7
// (index 6) fails twice and then loads, the rest are pictures drawn here.
// Every change the UI sends is kept: GET /__writes lists them, POST /__reset forgets them.

import http from 'node:http';

const PORT = 8787;
const PAGES = 10;
let writes = [];
let hits = {};

const chapter = (id, n = id) => ({
  id,
  name: `Chapter ${n}`,
  number: n,
  scanlator: null,
  dateUpload: 0,
  fetchedAt: 0,
  read: false,
  lastPageRead: 0,
  pageCount: null,
  downloaded: false,
  bookmark: false,
  mangaId: 1,
  volume: null,
  duplicate: false,
});

const item = (id) => ({
  id,
  title: `Test title ${id}`,
  cover: null,
  sourceName: 'Mock',
  status: 1,
  addedAt: 0,
  total: 5,
  unread: 3,
  downloaded: 1,
  lastReadAt: 0,
  latestAt: 0,
  categories: [],
});

const title = (id) => ({
  id,
  sourceId: '1',
  sourceName: 'Mock',
  sourceLang: 'en',
  webUrl: null,
  title: `Test title ${id}`,
  artist: null,
  author: 'Someone',
  description: 'A title made up for the test.',
  genres: [],
  status: 1,
  cover: null,
  inLibrary: true,
  addedAt: 0,
  initialized: true,
  readerMode: null,
  readerFit: null,
  autoDownload: false,
  chaptersAt: 0,
  releaseDays: null,
  categories: [],
  locked: [],
  scanlators: [],
  dedupe: false,
  mergedInto: null,
  members: [{ id, title: `Test title ${id}`, sourceName: 'Mock' }],
  // Newest first, as sources list them: chapters 1 and 2 are read, 3 is read to page 5.
  chapters: [5, 4, 3, 2, 1].map((n) => ({ ...chapter(n), mangaId: id, read: n < 3, lastPageRead: n === 3 ? 4 : 0, downloaded: n === 4 })),
});

const svg = (i) =>
  `<svg xmlns="http://www.w3.org/2000/svg" width="800" height="1200"><rect width="800" height="1200" fill="#fafafa"/>` +
  `<rect x="20" y="20" width="760" height="1160" fill="none" stroke="#111" stroke-width="8"/>` +
  `<text x="400" y="640" font-size="160" text-anchor="middle" font-family="sans-serif">${i + 1}</text></svg>`;

const json = (res, status, body) => {
  res.writeHead(status, { 'Content-Type': 'application/json' });
  res.end(JSON.stringify(body));
};

const GETS = [
  [/^\/api\/auth$/, () => ({ login: true, setup: false, setupCode: false, signup: false, signedIn: true, named: false, name: 'me', admin: true })],
  // The tour and the tap zones' first showing are taken as seen.
  [/^\/api\/settings$/, () => ({ 'tour.zones': true, 'tour.done': true })],
  [/^\/api\/library$/, () => ({ items: [item(1), item(2), item(3)], categories: [], resume: null })],
  [/^\/api\/categories$/, () => ({ categories: [] })],
  [/^\/api\/updates\/fresh$/, () => ({ fresh: 0 })],
  [/^\/api\/downloads$/, () => ({ queue: [], paused: false, held: false })],
  [
    /^\/api\/history$/,
    () => ({
      entries: [1, 2, 3].map((n) => ({ chapterId: n, chapterName: `Chapter ${n}`, readAt: 1794300000 - n * 100, page: 2, pageCount: PAGES, read: false, mangaId: 1, title: 'Test title 1', cover: null })),
    }),
  ],
  [/^\/api\/manga\/(\d+)$/, (m) => title(Number(m[1]))],
  [/^\/api\/manga\/\d+\/tracks$/, () => ({ trackers: [], tracks: [] })],
  [/^\/api\/manga\/\d+\/(marks|namesakes)$/, () => []],
  [
    /^\/api\/chapters\/(\d+)$/,
    (m) => ({
      chapter: chapter(Number(m[1])),
      mangaId: 1,
      title: 'Test title 1',
      kind: 'manga',
      readerMode: null,
      readerFit: null,
      readerPrefs: null,
      inLibrary: true,
      prev: null,
      next: null,
      chapters: [{ id: Number(m[1]), name: `Chapter ${m[1]}` }],
      marks: [],
    }),
  ],
  [/^\/api\/chapters\/\d+\/pages$/, () => ({ count: PAGES })],
];

http
  .createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://mock');
    const path = url.pathname;

    if (path === '/__writes') return json(res, 200, writes);
    if (path === '/__reset') {
      writes = [];
      hits = {};
      return json(res, 200, { ok: true });
    }

    if (req.method !== 'GET') {
      let body = '';
      req.on('data', (part) => (body += part));
      req.on('end', () => {
        writes.push({ method: req.method, path, body: body ? JSON.parse(body) : null });
        json(res, 200, { ok: true });
      });
      return;
    }

    const page = path.match(/^\/api\/chapters\/(\d+)\/pages\/(\d+)$/);
    if (page) {
      const i = Number(page[2]);
      const key = `${page[1]}:${i}`;
      hits[key] = (hits[key] ?? 0) + 1;
      if (i === 2) return json(res, 502, { error: { code: 'source', message: 'The source answered 503 Service Unavailable.' } });
      if (i === 6 && hits[key] <= 2) return json(res, 500, { error: { code: 'source', message: 'Flaky.' } });
      res.writeHead(200, { 'Content-Type': 'image/svg+xml', 'Cache-Control': 'no-store' });
      return res.end(svg(i));
    }

    for (const [pattern, answer] of GETS) {
      const m = path.match(pattern);
      if (m) return json(res, 200, answer(m));
    }
    json(res, 404, { error: { code: 'not_found', message: `The mock has no ${path}` } });
  })
  .listen(PORT, '127.0.0.1', () => console.log(`mock API on ${PORT}`));
