/**
 * The release notes of an offered version, from the signed manifest: plain text or a little
 * Markdown. They are read into a few kinds of block, never into HTML, so nothing in the notes can
 * add a link, an image or markup to the page.
 */

export type NotesBlock =
  | { kind: "heading"; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: string[] };

/** A part of a line: plain text, text in bold, or (only where links are allowed) a link. */
export interface NotesSpan {
  text: string;
  bold: boolean;
  /** An https address; set only when `spansOf` was asked for links. */
  href?: string;
}

const HEADING = /^#{1,6}\s+(.*)$/;
const BULLET = /^\s*(?:[-*+•]|\d+[.)])\s+(.*)$/;

/** The notes as headings, paragraphs and bullet lists, in order. Blank lines separate blocks. */
export function parseNotes(notes: string): NotesBlock[] {
  const blocks: NotesBlock[] = [];
  let paragraph: string[] = [];
  let list: string[] | null = null;
  const closeParagraph = () => {
    if (paragraph.length > 0) blocks.push({ kind: "paragraph", text: paragraph.join(" ") });
    paragraph = [];
  };
  const closeList = () => {
    if (list !== null && list.length > 0) blocks.push({ kind: "list", items: list });
    list = null;
  };
  for (const raw of notes.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trimEnd();
    if (line.trim() === "") {
      closeParagraph();
      closeList();
      continue;
    }
    const heading = HEADING.exec(line);
    if (heading !== null) {
      closeParagraph();
      closeList();
      blocks.push({ kind: "heading", text: heading[1]?.trim() ?? "" });
      continue;
    }
    const bullet = BULLET.exec(line);
    if (bullet !== null) {
      closeParagraph();
      list ??= [];
      list.push(bullet[1]?.trim() ?? "");
      continue;
    }
    // A line that continues the item above (indented) belongs to it; any other plain line ends it.
    if (list !== null && /^\s+\S/.test(raw)) {
      const last = list.length - 1;
      list[last] = `${list[last] ?? ""} ${line.trim()}`;
      continue;
    }
    closeList();
    paragraph.push(line.trim());
  }
  closeParagraph();
  closeList();
  return blocks;
}

const LINK = /\[([^\]]+)\]\((https:\/\/[^\s)]+)\)/g;

/** Bold and plain parts of a piece of text with no links in it. */
function boldSpans(text: string): NotesSpan[] {
  const spans: NotesSpan[] = [];
  const parts = text.split("**");
  // An odd number of markers leaves the last one unmatched: it is text, not bold.
  const closed = parts.length % 2 === 1 ? parts.length : parts.length - 1;
  parts.forEach((part, index) => {
    if (index < closed) {
      if (part !== "") spans.push({ text: part, bold: index % 2 === 1 });
    } else {
      spans.push({ text: (index === closed ? "**" : "") + part, bold: false });
    }
  });
  return spans;
}

/**
 * A line cut into plain and `**bold**` parts. An unclosed `**` stays as typed. With
 * `links` on, `[text](https://address)` also becomes a link span; only https addresses count,
 * anything else stays text. Notes from the update channel are read with links off.
 */
export function spansOf(text: string, options: { links?: boolean } = {}): NotesSpan[] {
  if (options.links !== true) return boldSpans(text);
  const spans: NotesSpan[] = [];
  let last = 0;
  for (const match of text.matchAll(LINK)) {
    spans.push(...boldSpans(text.slice(last, match.index)));
    spans.push({ text: match[1] ?? "", bold: false, href: match[2] ?? "" });
    last = match.index + match[0].length;
  }
  spans.push(...boldSpans(text.slice(last)));
  return spans;
}
