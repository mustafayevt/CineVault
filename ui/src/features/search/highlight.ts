/**
 * Matching letters of a suggestion: which stretches of a title the typed words hit, ignoring case
 * and accents, and how well the title fits. The backend decides what is a result (it forgives
 * typos); this only shows why, and tells an exact hit from a similar one.
 */

/** A half-open range of the original text. */
export interface Range {
  start: number;
  end: number;
}

const MARKS = /\p{M}/gu;

/** The text lower-cased without accents, and for each folded character where it came from. */
function fold(text: string): { folded: string; origin: number[] } {
  let folded = "";
  const origin: number[] = [];
  let index = 0;
  for (const char of text) {
    const plain = char.normalize("NFD").replace(MARKS, "").toLowerCase();
    for (const letter of plain) {
      folded += letter;
      origin.push(index);
    }
    index += char.length;
  }
  return { folded, origin };
}

/** The words of the typed text, folded and without the empty ones. */
function wordsOf(term: string): string[] {
  return fold(term)
    .folded.split(/[\s.:_-]+/u)
    .filter((word) => word !== "");
}

/** The stretches of `text` that the words of `term` hit, merged and in order. */
export function highlightRanges(text: string, term: string): Range[] {
  const words = wordsOf(term);
  if (words.length === 0) return [];
  const { folded, origin } = fold(text);
  const ranges: Range[] = [];
  for (const word of words) {
    let from = 0;
    for (;;) {
      const at = folded.indexOf(word, from);
      if (at === -1) break;
      const start = origin[at] ?? 0;
      const last = origin[at + word.length - 1] ?? start;
      // The original character at `last` may be several UTF-16 units wide.
      const end = last + (String.fromCodePoint(text.codePointAt(last) ?? 32).length || 1);
      ranges.push({ start, end });
      from = at + word.length;
    }
  }
  ranges.sort((a, b) => a.start - b.start || a.end - b.end);
  const merged: Range[] = [];
  for (const range of ranges) {
    const previous = merged.at(-1);
    if (previous !== undefined && range.start <= previous.end) {
      previous.end = Math.max(previous.end, range.end);
    } else {
      merged.push({ ...range });
    }
  }
  return merged;
}

/** `text` cut into pieces, each saying whether the typed words hit it. */
export function highlightParts(text: string, term: string): { text: string; hit: boolean }[] {
  const parts: { text: string; hit: boolean }[] = [];
  let at = 0;
  for (const range of highlightRanges(text, term)) {
    if (range.start > at) parts.push({ text: text.slice(at, range.start), hit: false });
    parts.push({ text: text.slice(range.start, range.end), hit: true });
    at = range.end;
  }
  if (at < text.length) parts.push({ text: text.slice(at), hit: false });
  return parts;
}

/**
 * How well a title fits the typed text: 100 the same, 80 it starts with it, 60 a word of it
 * does, 40 it contains it, 0 nothing in it matches (a result the backend found by forgiving a
 * typo).
 */
export function matchScore(text: string, term: string): number {
  const name = fold(text).folded;
  const typed = fold(term).folded.trim();
  if (typed === "") return 0;
  if (name === typed) return 100;
  if (name.startsWith(typed)) return 80;
  if (name.split(/[\s.:_-]+/u).some((word) => word.startsWith(typed))) return 60;
  if (name.includes(typed)) return 40;
  const words = wordsOf(term);
  return words.length > 1 && words.every((word) => name.includes(word)) ? 40 : 0;
}

/** True when no word of the typed text is in the title: the backend forgave a typo to list it. */
export function isSimilarOnly(text: string, term: string): boolean {
  return wordsOf(term).length > 0 && highlightRanges(text, term).length === 0;
}
