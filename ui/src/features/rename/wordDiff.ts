/**
 * The word-level diff of a rename: the folder name before and after, with the parts that went
 * away and the parts that came in marked. Whole words and the brackets around them stay
 * together, so `1984` becoming `(1984)` shows the whole `1984` struck and the whole `(1984)`
 * added, never a lone bracket or half a word.
 */

/** A run of text that is kept, or that went away (before) or came in (after). */
export interface DiffSegment {
  text: string;
  changed: boolean;
}

export interface WordDiff {
  /** The name as it was: the changed segments are the ones struck through. */
  before: DiffSegment[];
  /** The name as it will be: the changed segments are the ones highlighted. */
  after: DiffSegment[];
}

type TokenKind = "word" | "separator" | "open" | "close" | "other";

interface Token {
  text: string;
  kind: TokenKind;
  /** The opening bracket of the innermost group the token is in, or "" outside every group. */
  enclosure: string;
  /** The group the token is in, from 0, or -1. */
  group: number;
}

const WORD = /[\p{L}\p{N}]+(?:['’][\p{L}\p{N}]+)*/uy;
const SEPARATOR = /[\s._\-,/\\]+/uy;
const OPEN = /[([{]/uy;
const CLOSE = /[)\]}]/uy;
const MATCHING: Readonly<Record<string, string>> = { ")": "(", "]": "[", "}": "{" };

function matchAt(pattern: RegExp, text: string, at: number): string | null {
  pattern.lastIndex = at;
  const found = pattern.exec(text);
  return found === null ? null : found[0];
}

/** Splits a name into words, separators, brackets and single other characters, with groups. */
export function tokenize(name: string): Token[] {
  const tokens: Token[] = [];
  const open: { char: string; id: number }[] = [];
  let groups = 0;
  let at = 0;
  while (at < name.length) {
    const word = matchAt(WORD, name, at);
    const separator = word === null ? matchAt(SEPARATOR, name, at) : null;
    const opening = word === null && separator === null ? matchAt(OPEN, name, at) : null;
    const closing =
      word === null && separator === null && opening === null ? matchAt(CLOSE, name, at) : null;
    const top = open.at(-1);
    const base = { enclosure: top?.char ?? "", group: top?.id ?? -1 };
    if (word !== null) {
      tokens.push({ text: word, kind: "word", ...base });
      at += word.length;
    } else if (separator !== null) {
      tokens.push({ text: separator, kind: "separator", ...base });
      at += separator.length;
    } else if (opening !== null) {
      const id = groups;
      groups += 1;
      tokens.push({ text: opening, kind: "open", enclosure: opening, group: id });
      open.push({ char: opening, id });
      at += 1;
    } else if (closing !== null) {
      // A bracket that closes nothing is a plain character.
      if (top !== undefined && MATCHING[closing] === top.char) {
        tokens.push({ text: closing, kind: "close", enclosure: top.char, group: top.id });
        open.pop();
      } else {
        tokens.push({ text: closing, kind: "other", ...base });
      }
      at += 1;
    } else {
      const char = String.fromCodePoint(name.codePointAt(at) ?? 32);
      tokens.push({ text: char, kind: "other", ...base });
      at += char.length;
    }
  }
  // Brackets that never closed hold their words in no group.
  const closed = new Set(tokens.filter((token) => token.kind === "close").map((t) => t.group));
  return tokens.map((token) =>
    token.group >= 0 && !closed.has(token.group)
      ? { ...token, kind: token.kind === "open" ? "other" : token.kind, enclosure: "", group: -1 }
      : token,
  );
}

/** What two words must share to be the same word: the letters, without case, and the bracket. */
function wordKey(token: Token): string {
  return `${token.text.toLowerCase()}|${token.enclosure}`;
}

/** Pairs of word positions, in order, that both names hold (longest common subsequence). */
function commonWords(a: readonly Token[], b: readonly Token[]): [number, number][] {
  const wa = a.flatMap((token, index) => (token.kind === "word" ? [index] : []));
  const wb = b.flatMap((token, index) => (token.kind === "word" ? [index] : []));
  // The length of the longest common run of the words from x and y on, row by row.
  const width = wb.length + 1;
  const table = new Array<number>((wa.length + 1) * width).fill(0);
  const at = (x: number, y: number) => table[x * width + y] ?? 0;
  const same = (x: number, y: number) =>
    wordKey(a[wa[x] ?? 0] as Token) === wordKey(b[wb[y] ?? 0] as Token);
  for (let x = wa.length - 1; x >= 0; x -= 1) {
    for (let y = wb.length - 1; y >= 0; y -= 1) {
      table[x * width + y] = same(x, y)
        ? at(x + 1, y + 1) + 1
        : Math.max(at(x + 1, y), at(x, y + 1));
    }
  }
  const pairs: [number, number][] = [];
  let x = 0;
  let y = 0;
  while (x < wa.length && y < wb.length) {
    if (same(x, y)) {
      pairs.push([wa[x] ?? 0, wb[y] ?? 0]);
      x += 1;
      y += 1;
    } else if (at(x + 1, y) >= at(x, y + 1)) {
      x += 1;
    } else {
      y += 1;
    }
  }
  return pairs;
}

function textBetween(tokens: readonly Token[], from: number, to: number): string {
  return tokens
    .slice(from + 1, to)
    .map((token) => token.text)
    .join("");
}

/** Marks the characters between two kept words (or an end of the name) that differ. */
function markBetween(
  a: readonly Token[],
  b: readonly Token[],
  changedA: boolean[],
  changedB: boolean[],
  fromA: number,
  toA: number,
  fromB: number,
  toB: number,
): void {
  if (textBetween(a, fromA, toA) === textBetween(b, fromB, toB)) return;
  for (let index = fromA + 1; index < toA; index += 1) {
    const kind = a[index]?.kind;
    if (kind === "separator" || kind === "other") changedA[index] = true;
  }
  for (let index = fromB + 1; index < toB; index += 1) {
    const kind = b[index]?.kind;
    if (kind === "separator" || kind === "other") changedB[index] = true;
  }
}

function segments(tokens: readonly Token[], changed: readonly boolean[]): DiffSegment[] {
  const out: DiffSegment[] = [];
  tokens.forEach((token, index) => {
    const mark = changed[index] === true;
    const last = out.at(-1);
    if (last !== undefined && last.changed === mark) last.text += token.text;
    else out.push({ text: token.text, changed: mark });
  });
  return out;
}

/**
 * The words that stay, the words that go and the words that come, with the separators and
 * brackets between them. A word is kept when the other name has it too, whatever its case and
 * whether or not a bracket is around it in both. The separators between two kept words are marked
 * when they differ (`.` for a space), and a bracket pair is marked when every word inside it is.
 */
export function diffWords(before: string, after: string): WordDiff {
  const a = tokenize(before);
  const b = tokenize(after);
  const pairs = commonWords(a, b);
  const changedA = a.map((token) => token.kind === "word");
  const changedB = b.map((token) => token.kind === "word");
  for (const [x, y] of pairs) {
    changedA[x] = false;
    changedB[y] = false;
  }

  // Separators and the like: compare what stands between neighbouring kept words.
  const bounds: [number, number][] = [[-1, -1], ...pairs, [a.length, b.length]];
  for (let index = 0; index + 1 < bounds.length; index += 1) {
    const [fromA, fromB] = bounds[index] as [number, number];
    const [toA, toB] = bounds[index + 1] as [number, number];
    markBetween(a, b, changedA, changedB, fromA, toA, fromB, toB);
  }

  // A bracket pair goes with its words: struck or added together when all of them are.
  const settle = (tokens: readonly Token[], changed: boolean[]) => {
    const ids = new Set(tokens.filter((token) => token.kind === "open").map((t) => t.group));
    for (const id of ids) {
      const words = tokens.flatMap((token, index) =>
        token.group === id && token.kind === "word" ? [index] : [],
      );
      const whole = words.length > 0 && words.every((index) => changed[index] === true);
      tokens.forEach((token, index) => {
        if (token.group === id && (token.kind === "open" || token.kind === "close")) {
          changed[index] = whole;
        }
      });
    }
  };
  settle(a, changedA);
  settle(b, changedB);

  return { before: segments(a, changedA), after: segments(b, changedB) };
}
