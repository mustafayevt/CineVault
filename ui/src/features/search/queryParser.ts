import type { LibraryChipDto } from "../../bindings";
import { fieldByName, fieldById } from "./fields";
import type { FieldDef } from "./fields";

/**
 * The quick-search language.
 *
 * ```
 * hdr:dv year:2010..2019 -new actor:"mel gibson" heat
 * ```
 *
 * A term is a word, a `field:value` pair or either of them with a leading `-` (not). Words that are
 * not recognised are plain title search. Recognised terms become filter chips:
 *
 * - **Numbers** (`year`, `runtime`, `rating`, ...): `2015`, `2010..2019`, `2010..`, `..2000`,
 *   `>2010`, `>=2010`, `<2000`, `<=2000`.
 * - **Lists** (`hdr`, `genre`, `actor`, ...): one value or several separated by commas, any of which
 *   matches. Put a value in double quotes when it has spaces: `actor:"mel gibson"`.
 * - **Yes or no** (`new`, `review`, `locked`): `-new`, `is:new`, `new:no`. A bare
 *   `new` stays a title word, so a title like "New York" is still found.
 *
 * A known field with a value that makes no sense (`year:abc`, `hdr:xyz`) is *rejected*: it stays in
 * the text for editing, restricts nothing and is not searched as a title.
 */

/** A filter chip. It has the shape the backend takes, so it goes over IPC as it is. */
export type SearchChip = LibraryChipDto;

export type TermKind = "chip" | "free" | "rejected";

/** One whitespace-separated term of the input and what it was understood as. */
export interface Term {
  kind: TermKind;
  /** The term as typed, quotes included. */
  raw: string;
  /** Position of `raw` in the input. */
  start: number;
  end: number;
  /** Set for `chip`. */
  chip?: SearchChip;
  /** The title word, quotes removed. Set for `free`. */
  word?: string;
  /** The field name as typed, lower case. Set for `rejected`. */
  name?: string;
}

export interface ParsedQuery {
  terms: readonly Term[];
  /** The recognised terms, in order. */
  chips: readonly SearchChip[];
  /** Title words joined by spaces; what remains for the text search. */
  text: string;
  /** Terms of a known field whose value was refused. */
  rejected: readonly Term[];
  /** The input ends inside an unclosed quote: the term is not finished. */
  openQuote: boolean;
}

interface RawToken {
  raw: string;
  start: number;
  end: number;
}

/** Splits on whitespace outside double quotes. */
function tokenize(input: string): { tokens: RawToken[]; openQuote: boolean } {
  const tokens: RawToken[] = [];
  let start = -1;
  let inQuote = false;
  for (let i = 0; i < input.length; i += 1) {
    const ch = input.charAt(i);
    if (ch === '"') inQuote = !inQuote;
    const space = !inQuote && /\s/.test(ch);
    if (space) {
      if (start !== -1) tokens.push({ raw: input.slice(start, i), start, end: i });
      start = -1;
    } else if (start === -1) {
      start = i;
    }
  }
  if (start !== -1) tokens.push({ raw: input.slice(start), start, end: input.length });
  return { tokens, openQuote: inQuote };
}

/** Removes the quotes around a value; an unclosed opening quote is tolerated. */
function unquote(value: string): string {
  if (!value.startsWith('"')) return value;
  const close = value.indexOf('"', 1);
  return close === -1 ? value.slice(1) : value.slice(1, close);
}

/** Splits a list value on commas outside quotes and removes the quotes. */
function splitList(value: string): string[] {
  const parts: string[] = [];
  let current = "";
  let inQuote = false;
  for (const ch of value) {
    if (ch === '"') {
      inQuote = !inQuote;
    } else if (ch === "," && !inQuote) {
      parts.push(current);
      current = "";
    } else {
      current += ch;
    }
  }
  parts.push(current);
  return parts.map((part) => part.trim()).filter((part) => part !== "");
}

const YES = new Set(["yes", "true", "1", "y"]);
const NO = new Set(["no", "false", "0", "n"]);

const NUMBER = /^\d+(?:\.\d+)?$/;

function toNumber(text: string): number | null {
  return NUMBER.test(text) ? Number(text) : null;
}

/** Rounds to one decimal, so `7 + 0.1` is `7.1` and not `7.1000000000000005`. */
function oneDecimal(value: number): number {
  return Math.round(value * 10) / 10;
}

interface Range {
  min: number | null;
  max: number | null;
}

/** Reads `5`, `5..9`, `5..`, `..9`, `>5`, `>=5`, `<9`, `<=9`; `null` when it is none of them. */
function parseRange(text: string, def: FieldDef): Range | null {
  const step = def.kind === "decimal" ? 0.1 : 1;
  const comparison = /^(>=|<=|>|<)(.+)$/.exec(text);
  if (comparison !== null) {
    const n = toNumber((comparison[2] ?? "").trim());
    if (n === null) return null;
    switch (comparison[1]) {
      case ">=":
        return { min: n, max: null };
      case "<=":
        return { min: null, max: n };
      case ">":
        return { min: oneDecimal(n + step), max: null };
      default:
        return { min: null, max: oneDecimal(n - step) };
    }
  }
  const dots = text.indexOf("..");
  if (dots !== -1) {
    const left = text.slice(0, dots).trim();
    const right = text.slice(dots + 2).trim();
    if (left === "" && right === "") return null;
    const min = left === "" ? null : toNumber(left);
    const max = right === "" ? null : toNumber(right);
    if ((left !== "" && min === null) || (right !== "" && max === null)) return null;
    return { min, max };
  }
  const n = toNumber(text);
  return n === null ? null : { min: n, max: n };
}

function rangeIsValid(range: Range, def: FieldDef): boolean {
  const { min, max } = range;
  if (min !== null && max !== null && min > max) return false;
  const bounds = def.bounds;
  if (bounds === undefined) return true;
  for (const value of [min, max]) {
    if (value !== null && (value < bounds.min || value > bounds.max)) return false;
  }
  if (def.kind === "integer") {
    return [min, max].every((value) => value === null || Number.isInteger(value));
  }
  return true;
}

function chip(def: FieldDef, negated: boolean, rest: Partial<SearchChip>): SearchChip {
  return { field: def.id, negated, values: [], min: null, max: null, ...rest };
}

/** The chip for `field:value`, or `null` when the value is not valid for the field. */
function chipFromValue(def: FieldDef, negated: boolean, value: string): SearchChip | null {
  switch (def.kind) {
    case "flag": {
      const word = unquote(value).toLowerCase();
      if (YES.has(word)) return chip(def, negated, {});
      if (NO.has(word)) return chip(def, !negated, {});
      return null;
    }
    case "integer":
    case "decimal": {
      const range = parseRange(unquote(value).trim(), def);
      if (range === null || !rangeIsValid(range, def)) return null;
      return chip(def, negated, range);
    }
    case "list": {
      const values: string[] = [];
      for (const part of splitList(value)) {
        const normal = def.normalize === undefined ? part : def.normalize(part);
        if (normal === null) return null;
        if (!values.includes(normal)) values.push(normal);
      }
      return values.length === 0 ? null : chip(def, negated, { values });
    }
  }
}

function classify(token: RawToken): Term {
  const base = { raw: token.raw, start: token.start, end: token.end };
  const free = (): Term => ({ ...base, kind: "free", word: unquote(token.raw) });

  let body = token.raw;
  let negated = false;
  if (body.length > 1 && body.startsWith("-")) {
    negated = true;
    body = body.slice(1);
  }
  const colon = body.indexOf(":");
  const quote = body.indexOf('"');
  if (colon === -1 || (quote !== -1 && quote < colon)) {
    // `-new`: a bare yes/no word is a chip only with its minus.
    const def = negated ? fieldByName(body) : undefined;
    if (def?.kind === "flag") return { ...base, kind: "chip", chip: chip(def, true, {}) };
    return free();
  }

  const name = body.slice(0, colon).toLowerCase();
  const value = body.slice(colon + 1);
  if (name === "is") {
    // `is:new`, `-is:new`
    const def = fieldByName(unquote(value));
    if (def?.kind === "flag") return { ...base, kind: "chip", chip: chip(def, negated, {}) };
    return { ...base, kind: "rejected", name };
  }
  const def = fieldByName(name);
  if (def === undefined) return free();
  const parsed = value === "" ? null : chipFromValue(def, negated, value);
  if (parsed === null) return { ...base, kind: "rejected", name };
  return { ...base, kind: "chip", chip: parsed };
}

/** Understands the quick-search text. */
export function parseQuery(input: string): ParsedQuery {
  const { tokens, openQuote } = tokenize(input);
  const terms = tokens.map(classify);
  return {
    terms,
    chips: terms.flatMap((term) => (term.chip === undefined ? [] : [term.chip])),
    text: terms
      .flatMap((term) => (term.word === undefined || term.word === "" ? [] : [term.word]))
      .join(" "),
    rejected: terms.filter((term) => term.kind === "rejected"),
    openQuote,
  };
}

function quoteIfNeeded(value: string): string {
  return /[\s,":]/.test(value) || value.startsWith("-") ? `"${value.replaceAll('"', "")}"` : value;
}

function numberText(value: number): string {
  return String(value);
}

/** The quick-search term for a chip; parsing it gives the chip back. */
export function chipToTerm(source: SearchChip): string {
  const def = fieldById(source.field);
  const minus = source.negated ? "-" : "";
  const name = def.names[0] ?? source.field;
  switch (def.kind) {
    case "flag":
      return source.negated ? `-${name}` : `is:${name}`;
    case "integer":
    case "decimal": {
      const { min, max } = source;
      let value: string;
      if (min !== null && max !== null) {
        value = min === max ? numberText(min) : `${numberText(min)}..${numberText(max)}`;
      } else if (min !== null) {
        value = `>=${numberText(min)}`;
      } else if (max !== null) {
        value = `<=${numberText(max)}`;
      } else {
        value = "";
      }
      return `${minus}${name}:${value}`;
    }
    case "list":
      return `${minus}${name}:${source.values.map(quoteIfNeeded).join(",")}`;
  }
}

/** The text that parses back to these chips and this title text. */
export function formatQuery(chips: readonly SearchChip[], text: string): string {
  return [...chips.map(chipToTerm), text.trim()].filter((part) => part !== "").join(" ");
}

/**
 * Moves the recognised terms of `input` into chips. What is left (title words, rejected terms) is
 * the text to keep in the field, each term as typed.
 */
export function extractChips(input: string): { chips: SearchChip[]; rest: string } {
  const parsed = parseQuery(input);
  const kept = parsed.terms.filter((term) => term.kind !== "chip").map((term) => term.raw);
  return { chips: [...parsed.chips], rest: kept.join(" ") };
}

/** `input` without the term at `start..end`, keeping one space between its neighbours. */
export function removeTerm(input: string, start: number, end: number): string {
  const before = input.slice(0, start).trimEnd();
  const after = input.slice(end).trimStart();
  return before !== "" && after !== "" ? `${before} ${after}` : `${before}${after}`;
}
