// Markdown in a title's description, as some sources write it. The text is escaped
// before anything else, so no tag a source sends reaches the page: only the tags
// made here do. Links go out only to web and mail addresses, and pictures are not
// loaded: an image becomes a link to it.

const ESCAPES: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
const UNESCAPES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', '#39': "'" };

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ESCAPES[c]);
}

const unescapeHtml = (html: string) => html.replace(/&(amp|lt|gt|quot|#39);/g, (_, name: string) => UNESCAPES[name]);

/** A link's address, already escaped. Only web and mail addresses are kept. */
const safeUrl = (url: string) => (/^(https?:\/\/|mailto:)/i.test(url) ? url : null);

const anchor = (href: string, text: string) => `<a href="${href}" target="_blank" rel="noreferrer noopener nofollow">${text}</a>`;

/** Bold, italics, struck text, code and links, in one line of escaped text. */
function inline(text: string): string {
  // What is made first is set aside, so the rules after it leave it alone.
  const held: string[] = [];
  const hold = (html: string) => `\u0000${held.push(html) - 1}\u0000`;
  const out = text
    .replace(/`([^`]+)`/g, (_, code: string) => hold(`<code>${code}</code>`))
    .replace(/!?\[([^\]]+)\]\(([^)\s]+)(?:\s+&quot;[^)]*&quot;)?\)/g, (_, label: string, url: string) => {
      const href = safeUrl(url);
      return href ? hold(anchor(href, label)) : label;
    })
    // A bare address, without what usually follows one in a sentence.
    .replace(/(^|[\s(])(https?:\/\/(?:[^\s<&]|&amp;)+)/gi, (_, lead: string, url: string) => {
      const tail = url.match(/[.,:;!?)\]]+$/)?.[0] ?? '';
      const bare = url.slice(0, url.length - tail.length);
      return lead + hold(anchor(bare, bare)) + tail;
    })
    .replace(/\*\*(?=\S)([^*]+?)\*\*/g, '<strong>$1</strong>')
    .replace(/(^|\W)__(?=\S)(.+?)__(?=\W|$)/g, '$1<strong>$2</strong>')
    .replace(/(^|[^*\w])\*(?=\S)([^*]+?)\*(?!\*)/g, '$1<em>$2</em>')
    .replace(/(^|\W)_(?=\S)([^_]+?)_(?=\W|$)/g, '$1<em>$2</em>')
    .replace(/~~(?=\S)(.+?)~~/g, '<del>$1</del>');
  // A link's label can hold code: put back twice.
  return out.replace(/\u0000(\d+)\u0000/g, (_, n: string) => held[Number(n)]).replace(/\u0000(\d+)\u0000/g, (_, n: string) => held[Number(n)]);
}

type Kind = 'p' | 'quote' | 'ul' | 'ol' | 'code';

const BULLET = /^\s*[-*+]\s+(.*)$/;
const NUMBER = /^\s*\d{1,3}[.)]\s+(.*)$/;
const HEADING = /^(#{1,6})\s+(.*?)\s*#*\s*$/;
const RULE = /^\s*([-*_])(\s*\1){2,}\s*$/;
const QUOTE = /^\s*&gt;\s?(.*)$/;
const FENCE = /^\s*```/;

/** The description as HTML. Text without Markdown comes out as its paragraphs and line breaks. */
export function markdown(source: string): string {
  const out: string[] = [];
  let kind = null as Kind | null;
  let lines: string[] = [];

  const close = () => {
    if (kind === 'code') out.push(`<pre><code>${lines.join('\n')}</code></pre>`);
    else if (kind === 'p') out.push(`<p>${lines.map(inline).join('<br>')}</p>`);
    else if (kind === 'quote') out.push(`<blockquote>${markdown(unescapeHtml(lines.join('\n')))}</blockquote>`);
    else if (kind) out.push(`<${kind}>${lines.map((line) => `<li>${inline(line)}</li>`).join('')}</${kind}>`);
    kind = null;
    lines = [];
  };
  const into = (next: Kind, line: string) => {
    if (kind !== next) {
      close();
      kind = next;
    }
    lines.push(line);
  };

  for (const line of escapeHtml(source.replace(/\r\n?/g, '\n')).split('\n')) {
    if (kind === 'code') {
      if (FENCE.test(line)) close();
      else lines.push(line);
      continue;
    }
    let m: RegExpMatchArray | null;
    if (FENCE.test(line)) {
      close();
      kind = 'code';
    } else if (!line.trim()) close();
    else if ((m = line.match(HEADING))) {
      close();
      // The title is the page's own heading: these sit below it.
      const level = Math.min(6, m[1].length + 2);
      out.push(`<h${level}>${inline(m[2])}</h${level}>`);
    } else if (RULE.test(line)) {
      close();
      out.push('<hr>');
    } else if ((m = line.match(QUOTE))) into('quote', m[1]);
    else if ((m = line.match(BULLET))) into('ul', m[1]);
    else if ((m = line.match(NUMBER))) into('ol', m[1]);
    else if ((kind === 'ul' || kind === 'ol') && /^\s{2,}\S/.test(line)) lines[lines.length - 1] += ' ' + line.trim();
    else into('p', line);
  }
  close();
  return out.join('');
}
