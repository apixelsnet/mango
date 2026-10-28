export function hash(text: string): number {
  let h = 0;
  for (let i = 0; i < text.length; i++) h = (h * 31 + text.charCodeAt(i)) >>> 0;
  return h;
}

/**
 * Puts text on the clipboard. Browsers offer the clipboard only to HTTPS pages and this
 * machine's own, so a Mango opened by its address on the network copies the old way.
 */
export async function copyText(text: string): Promise<boolean> {
  if (navigator.clipboard && window.isSecureContext) {
    return navigator.clipboard.writeText(text).then(
      () => true,
      () => false,
    );
  }
  const field = document.createElement('textarea');
  field.value = text;
  field.setAttribute('readonly', '');
  field.style.position = 'fixed';
  field.style.opacity = '0';
  document.body.append(field);
  field.select();
  try {
    return document.execCommand('copy');
  } catch {
    return false;
  } finally {
    field.remove();
  }
}

/** "3 h ago", from a time in seconds. */
export function ago(seconds: number): string {
  const diff = Math.max(0, Date.now() / 1000 - seconds);
  if (diff < 60) return 'just now';
  if (diff < 3600) return `${Math.floor(diff / 60)} min ago`;
  if (diff < 86400) return `${Math.floor(diff / 3600)} h ago`;
  if (diff < 86400 * 30) return `${Math.floor(diff / 86400)} d ago`;
  return new Date(seconds * 1000).toLocaleDateString(undefined, { year: 'numeric', month: 'short', day: 'numeric' });
}

/** A heading for a day: Today, Yesterday, or the date. From a time in seconds. */
export function dayLabel(seconds: number): string {
  const day = new Date(seconds * 1000);
  const start = (d: Date) => new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime();
  const days = Math.round((start(new Date()) - start(day)) / 86_400_000);
  if (days === 0) return 'Today';
  if (days === 1) return 'Yesterday';
  return day.toLocaleDateString(undefined, { weekday: 'long', month: 'long', day: 'numeric', year: days > 300 ? 'numeric' : undefined });
}

/** Seconds since the epoch at the start of that day, to group by. */
export function dayKey(seconds: number): number {
  const d = new Date(seconds * 1000);
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime() / 1000;
}

export const STATUS = ['Unknown', 'Ongoing', 'Completed', 'Licensed', 'Finished', 'Cancelled', 'On hiatus'];

const names = typeof Intl.DisplayNames === 'function' ? new Intl.DisplayNames(undefined, { type: 'language' }) : null;

export function langName(code: string): string {
  if (code === 'all') return 'Multi';
  if (code === 'other') return 'Other';
  try {
    return names?.of(code) ?? code;
  } catch {
    return code;
  }
}

/** "Ch. 12.5" when the number is known, else the chapter's own name. */
export function chapterShort(chapter: { number: number; name: string }): string {
  return chapter.number >= 0 ? `Ch. ${+chapter.number.toFixed(2)}` : chapter.name;
}

/** Runs `run` each time the element comes near the viewport. For lists that load as they are scrolled. */
export function whenVisible(node: HTMLElement, run: () => void) {
  let current = run;
  const observer = new IntersectionObserver((entries) => entries.some((e) => e.isIntersecting) && current(), { rootMargin: '800px' });
  observer.observe(node);
  return {
    update(next: () => void) {
      current = next;
    },
    destroy() {
      observer.disconnect();
    },
  };
}

/**
 * Runs `run` every `ms` while the page is in view, and once more when it comes back into
 * view. A tab left open in the background then asks Mango nothing: on a hosted Mango that
 * lets an idle instance sleep. Returns what stops it.
 */
export function every(ms: number, run: () => void): () => void {
  let timer: ReturnType<typeof setInterval> | undefined;
  const start = () => {
    clearInterval(timer);
    timer = document.hidden ? undefined : setInterval(run, ms);
  };
  const back = () => {
    if (!document.hidden) run();
    start();
  };
  start();
  document.addEventListener('visibilitychange', back);
  return () => {
    clearInterval(timer);
    document.removeEventListener('visibilitychange', back);
  };
}
