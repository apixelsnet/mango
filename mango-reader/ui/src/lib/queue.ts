import { get, type Queue, type Queued } from './api';

/** Why the queue waits, if it does: paused by the reader, or held while storage is full. */
export type Waits = '' | 'paused' | 'held';

export type QueueOf = { entries: Record<number, Queued>; waits: Waits };

/** The queue's entries for these chapters, by chapter. Null when the queue could not be read. */
export async function queueOf(chapters: Iterable<number>): Promise<QueueOf | null> {
  const answer = await get<Queue>('/downloads').catch(() => null);
  if (!answer) return null;
  const ids = new Set(chapters);
  return {
    entries: Object.fromEntries(answer.queue.filter((q) => ids.has(q.chapterId)).map((q) => [q.chapterId, q])),
    waits: answer.held ? 'held' : answer.paused ? 'paused' : '',
  };
}

/** Whether a chapter that was in `before` is gone from `after`: it was saved, or taken out elsewhere. */
export function left(before: Record<number, Queued>, after: Record<number, Queued>): boolean {
  return Object.keys(before).some((chapter) => !after[+chapter]);
}

/** Whether one of these can still change by itself, so the queue is worth asking again. */
export const moving = (entries: Record<number, Queued>) => Object.values(entries).some((q) => q.state !== 'failed');

/** A chapter shown as queued at once, before the queue itself is read. */
export function waiting(chapterId: number, chapterName: string, mangaId: number, title: string): Queued {
  return { chapterId, chapterName, mangaId, title, state: 'queued', done: 0, total: null, attempts: 0, retryAt: null, error: null };
}
