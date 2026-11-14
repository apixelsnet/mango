// The description renderer's output goes into the title page as HTML, so whatever a source
// writes must come out as text, and only web and mail links may come out as links. No
// browser is needed: these run in Node, beside the browser tests.

import { expect, test } from '@playwright/test';
import { escapeHtml, markdown } from '../src/lib/markdown';

/** Every tag the output holds, by name. */
const tags = (html: string) => [...html.matchAll(/<([a-z0-9]+)[\s>]/gi)].map((m) => m[1].toLowerCase());
/** Every address a link in the output goes to. */
const hrefs = (html: string) => [...html.matchAll(/href="([^"]*)"/g)].map((m) => m[1]);

const ALLOWED = new Set(['p', 'br', 'strong', 'em', 'del', 'code', 'pre', 'a', 'ul', 'ol', 'li', 'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr']);

test.describe('description Markdown', () => {
  test('a tag a source sends is text', () => {
    for (const hostile of [
      '<script>alert(1)</script>',
      '<img src=x onerror=alert(1)>',
      '<iframe src="https://example.org"></iframe>',
      '**<b onmouseover=alert(1)>bold</b>**',
      '> <svg onload=alert(1)>',
      '```\n<script>alert(1)</script>\n```',
    ]) {
      const html = markdown(hostile);
      expect(tags(html).filter((tag) => !ALLOWED.has(tag)), hostile).toEqual([]);
      expect(html).not.toMatch(/<(script|img|iframe|svg|b)\b/i);
    }
  });

  test('only web and mail addresses become links', () => {
    const html = markdown(
      [
        '[web](https://example.org/a) [mail](mailto:someone@example.org)',
        '[js](javascript:alert(1)) [data](data:text/html,hi) [vb](vbscript:x) [rel](/api/settings)',
        '![cover](https://example.org/c.jpg)',
        'Bare https://example.org/b, then text.',
      ].join('\n'),
    );
    expect(hrefs(html)).toEqual(['https://example.org/a', 'mailto:someone@example.org', 'https://example.org/c.jpg', 'https://example.org/b']);
    expect(html).toContain('js');
    // A picture is never loaded: it is a link to it.
    expect(tags(html)).not.toContain('img');
  });

  test('a link cannot break out of its address', () => {
    const html = markdown('[x](https://example.org/"onmouseover="alert(1))');
    for (const href of hrefs(html)) expect(href).not.toContain('"');
    expect(html).not.toMatch(/\sonmouseover=/i);
    for (const anchor of html.match(/<a [^>]*>/g) ?? []) {
      expect(anchor).toMatch(/^<a href="[^"]*" target="_blank" rel="noreferrer noopener nofollow">$/);
    }
  });

  test('plain text keeps its paragraphs and lines', () => {
    expect(markdown('One\ntwo\n\nThree')).toBe('<p>One<br>two</p><p>Three</p>');
    expect(markdown('a < b & c')).toBe('<p>a &lt; b &amp; c</p>');
  });

  test('the usual marks come out', () => {
    expect(markdown('**bold** *it* ~~gone~~ `code`')).toBe('<p><strong>bold</strong> <em>it</em> <del>gone</del> <code>code</code></p>');
    expect(markdown('- one\n- two')).toBe('<ul><li>one</li><li>two</li></ul>');
    expect(markdown('1. one\n2. two')).toBe('<ol><li>one</li><li>two</li></ol>');
  });

  test('escaping covers every character that matters', () => {
    expect(escapeHtml(`<a href="x" title='y'>&</a>`)).toBe('&lt;a href=&quot;x&quot; title=&#39;y&#39;&gt;&amp;&lt;/a&gt;');
  });
});
