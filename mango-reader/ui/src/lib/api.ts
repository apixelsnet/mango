// Calls to Mango's API, and the shapes it answers with.

export class ApiError extends Error {
  constructor(
    public status: number,
    public code: string,
    message: string,
  ) {
    super(message);
  }
}

type Options = { method?: string; json?: unknown; signal?: AbortSignal };

/** Told when Mango answers that nobody is signed in. The session store listens. */
export const gate = { lost: (_code: string) => {} };

export async function api<T>(path: string, options: Options = {}): Promise<T> {
  let response: Response;
  try {
    response = await fetch('api' + path, {
      method: options.method ?? 'GET',
      signal: options.signal,
      headers: options.json === undefined ? undefined : { 'Content-Type': 'application/json' },
      body: options.json === undefined ? undefined : JSON.stringify(options.json),
    });
  } catch {
    throw new ApiError(0, 'offline', 'Mango did not answer. Is it still running?');
  }
  let data: any = null;
  try {
    data = await response.json();
  } catch {
    // An answer without JSON: only the status says anything.
  }
  if (!response.ok) {
    const code = data?.error?.code;
    if (response.status === 401 && (code === 'signed_out' || code === 'setup_required')) gate.lost(code);
    throw new ApiError(response.status, data?.error?.code ?? 'error', data?.error?.message ?? `Mango answered ${response.status}`);
  }
  return data as T;
}

export const get = <T>(path: string, signal?: AbortSignal) => api<T>(path, { signal });
export const send = <T = unknown>(method: string, path: string, json?: unknown) => api<T>(path, { method, json });

/** A sentence for the screen, whatever was thrown. */
export function message(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

export type Release = {
  version: string;
  name: string;
  notes: string;
  url: string;
  publishedAt: string | null;
  prerelease: boolean;
  /** The file for the platform Mango runs on, when the release has one. */
  download: { name: string; url: string; size: number } | null;
};

export type VersionInfo = {
  current: string;
  commit: string;
  platform: string;
  docker: boolean;
  /** Whether this Mango knows where its releases are listed. */
  canCheck: boolean;
  checkHours: number;
  checkedAt: number | null;
  error: string | null;
  updateAvailable: boolean;
  latest: Release | null;
  releases: Release[];
};

export type Card = { id: number; sourceId: string; title: string; cover: string | null; inLibrary: boolean };
export type Listing = { items: Card[]; hasNextPage: boolean };

export type Chapter = {
  id: number;
  name: string;
  number: number;
  scanlator: string | null;
  dateUpload: number;
  fetchedAt: number;
  read: boolean;
  lastPageRead: number;
  pageCount: number | null;
  downloaded: boolean;
};

export type Title = {
  id: number;
  sourceId: string;
  sourceName: string | null;
  sourceLang: string | null;
  webUrl: string | null;
  title: string;
  artist: string | null;
  author: string | null;
  description: string | null;
  genres: string[];
  status: number;
  cover: string | null;
  inLibrary: boolean;
  addedAt: number | null;
  initialized: boolean;
  readerMode: string | null;
  readerFit: string | null;
  autoDownload: boolean;
  chaptersAt: number | null;
  categories: number[];
  chapters: Chapter[];
};

/** One of the two titles of a migration. */
export type MigrateSide = {
  id: number;
  title: string;
  cover: string | null;
  sourceName: string | null;
  sourceLang: string | null;
  inLibrary: boolean;
  chapters: number;
  read: number;
  /** The highest chapter number it has, and the highest that is read. -1 when there is none. */
  latest: number;
  lastRead: number;
  downloaded: number;
};

/** What moving `from` onto `to` would carry over. */
export type Comparison = { from: MigrateSide; to: MigrateSide; read: number; categories: number; tracks: number };

export type LibraryItem = {
  id: number;
  title: string;
  cover: string | null;
  sourceName: string | null;
  status: number;
  addedAt: number;
  total: number;
  unread: number;
  downloaded: number;
  lastReadAt: number;
  latestAt: number;
  categories: number[];
};

export type Category = { id: number; name: string };

export type Resume = {
  mangaId: number;
  title: string;
  cover: string | null;
  chapterId: number;
  chapterName: string;
  page: number;
  pageCount: number | null;
};

export type Library = { items: LibraryItem[]; categories: Category[]; resume: Resume | null };

export type HistoryEntry = {
  chapterId: number;
  chapterName: string;
  readAt: number;
  page: number;
  pageCount: number | null;
  read: boolean;
  mangaId: number;
  title: string;
  cover: string | null;
};

export type UpdateItem = {
  chapterId: number;
  chapterName: string;
  fetchedAt: number;
  read: boolean;
  downloaded: boolean;
  mangaId: number;
  title: string;
  cover: string | null;
  sourceName: string | null;
};

export type Updates = {
  items: UpdateItem[];
  errors: { mangaId: number; title: string; error: string; at: number }[];
  /** Unread chapters found in the last week. */
  fresh: number;
  status: { running: boolean; done: number; total: number; found: number; failed: number; startedAt: number | null; finishedAt: number | null };
};

export type Queued = {
  chapterId: number;
  chapterName: string;
  mangaId: number;
  title: string;
  state: 'queued' | 'downloading' | 'failed';
  done: number;
  total: number | null;
  attempts: number;
  retryAt: number | null;
  error: string | null;
};

export type Queue = { paused: boolean; queue: Queued[] };

export type ReaderInfo = {
  chapter: Chapter;
  mangaId: number;
  title: string;
  /** What sort of series the tags and the source make it; see lib/reader.ts. */
  kind: 'manga' | 'manhwa' | 'manhua' | 'comic';
  readerMode: string | null;
  readerFit: string | null;
  readerPrefs: Record<string, unknown> | null;
  prev: number | null;
  next: number | null;
  chapters: { id: number; name: string }[];
};

export type Tracker = {
  id: number;
  name: string;
  /** How one signs in: a token pasted in, a trip to the site and back, or a name and password. */
  login: 'token' | 'oauth' | 'password';
  signedIn: boolean;
  userName: string | null;
  statuses: { value: number; label: string }[];
  /** Scores show out of ten: what a link stores is the shown score times this. */
  scoreScale: number;
  scoreStep: number;
};

/** A title's link to a tracker. */
export type Track = {
  tracker: number;
  libraryId: number;
  mediaId: number;
  url: string;
  title: string;
  lastChapterRead: number;
  /** 0 when the tracker does not know. */
  totalChapters: number;
  score: number;
  status: number;
  /** Set while the last change has not reached the tracker. */
  error: string | null;
};

export type TrackingView = { trackers: Tracker[]; tracks: Track[] };

/** A title as a tracker's search lists it. */
export type TrackerMatch = { mediaId: number; title: string; summary: string; kind: string; year: number | null; totalChapters: number; url: string };

export type Source = {
  id: string;
  pkg: string;
  name: string;
  lang: string;
  baseUrl: string | null;
  supportsLatest: boolean;
  pinned: boolean;
  hidden: boolean;
};

/** One thing a source lets its user set. `value` is a boolean for a switch, text for a list or a text, a list of texts for a multi. */
export type SourcePref = {
  position: number;
  type: 'switch' | 'list' | 'multi' | 'text' | 'info';
  key: string | null;
  title: string | null;
  summary: string | null;
  enabled: boolean;
  visible: boolean;
  value?: boolean | string | string[] | null;
  /** What a list or a multi offers: the words shown, and the values kept, in the same order. */
  entries?: string[];
  entryValues?: string[];
  dialogMessage?: string | null;
};

export type Store = {
  id: number;
  url: string;
  indexUrl: string;
  name: string;
  website: string | null;
  signingKey: string;
  keyChanged: boolean;
  offeredKey: string | null;
  extensions: number;
  checkedAt: number | null;
  error: string | null;
};

export type CatalogItem = {
  storeId: number;
  pkg: string;
  name: string;
  versionName: string;
  lib: string;
  contentWarning: number;
  languages: string[];
  sourceCount: number;
  installable: boolean;
  installed: boolean;
  installedVersion: string | null;
  updateAvailable: boolean;
};

export type Installed = {
  pkg: string;
  name: string;
  versionName: string;
  lib: string;
  contentWarning: number;
  storeId: number | null;
  updateAvailable: boolean;
  availableVersion: string | null;
  sources: Source[];
};
