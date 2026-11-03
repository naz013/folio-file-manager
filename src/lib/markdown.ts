/**
 * Release notes are GitHub Markdown. This reads the small part of it they use (headings,
 * bullets, paragraphs, bold, code and links) into plain data, so the page draws it with
 * its own elements and nothing from the notes is ever inserted as HTML.
 */

export interface Span {
  text: string;
  bold?: boolean;
  code?: boolean;
  /** Only an http(s) address; anything else is left as plain text. */
  href?: string;
}

export type Block =
  | { kind: "heading"; level: 1 | 2 | 3; spans: Span[] }
  | { kind: "list"; items: Span[][] }
  | { kind: "paragraph"; spans: Span[] };

const INLINE = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\(([^)\s]+)\)|(https?:\/\/[^\s<>)]+)/g;

const isWeb = (url: string) => /^https?:\/\//i.test(url);

export function inline(text: string): Span[] {
  const spans: Span[] = [];
  let last = 0;
  for (const m of text.matchAll(INLINE)) {
    if (m.index > last) spans.push({ text: text.slice(last, m.index) });
    if (m[1] !== undefined) spans.push({ text: m[1], bold: true });
    else if (m[2] !== undefined) spans.push({ text: m[2], code: true });
    else if (m[3] !== undefined) {
      spans.push(isWeb(m[4]) ? { text: m[3], href: m[4] } : { text: m[3] });
    } else {
      // A sentence can end right after an address.
      const url = m[5].replace(/[.,;:!?]+$/, "");
      spans.push({ text: url, href: url });
      if (url.length < m[5].length) spans.push({ text: m[5].slice(url.length) });
    }
    last = m.index + m[0].length;
  }
  if (last < text.length) spans.push({ text: text.slice(last) });
  return spans;
}

export function parseMarkdown(source: string): Block[] {
  const blocks: Block[] = [];
  let paragraph: string[] = [];
  let list: Span[][] | null = null;

  const flush = () => {
    if (paragraph.length) blocks.push({ kind: "paragraph", spans: inline(paragraph.join(" ")) });
    if (list) blocks.push({ kind: "list", items: list });
    paragraph = [];
    list = null;
  };

  for (const raw of source.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    const heading = /^(#{1,6})\s+(.*)$/.exec(line);
    const bullet = /^(?:[-*+]|\d+[.)])\s+(.*)$/.exec(line);
    // Rules and code fences carry no words of their own.
    if (/^(?:-{3,}|\*{3,}|_{3,}|```.*)$/.test(line)) flush();
    else if (!line) flush();
    else if (heading) {
      flush();
      const level = Math.min(heading[1].length, 3) as 1 | 2 | 3;
      blocks.push({ kind: "heading", level, spans: inline(heading[2]) });
    } else if (bullet) {
      if (paragraph.length) flush();
      (list ??= []).push(inline(bullet[1]));
    } else {
      if (list) flush();
      paragraph.push(line);
    }
  }
  flush();
  return blocks;
}
