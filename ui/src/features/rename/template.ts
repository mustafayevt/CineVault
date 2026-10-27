/**
 * Naming templates as chips.
 *
 * A template is text such as `${title} (${year})` or `${title}${ - ,edition,} (${year})`. The chip
 * editor shows each `${...}` as a chip between pieces of literal text. The model keeps the exact
 * source of every chip, so turning text into chips and back never changes a character: the raw
 * mode round trip is lossless for any text, including expressions the chip view does not
 * understand (those become "advanced" chips that show their source).
 */

/** What a `${...}` expression is, as far as the chip view is concerned. */
export type TokenKind =
  /** `${name}` or `${name;renderer}`. */
  | "token"
  /** `${prefix,name,suffix}`: the name with text around it that disappears when it is empty. */
  | "optional"
  /** Anything else: `${if x}`, `${end}`, `${foreach ...}`. Shown as its source. */
  | "advanced";

export interface TemplateToken {
  /** The exact text, `${` to `}`. */
  source: string;
  kind: TokenKind;
  /** The token name for `token` and `optional`; empty for `advanced`. */
  name: string;
  /** Text before the value, for `optional`. */
  prefix: string;
  /** Text after the value, for `optional`. */
  suffix: string;
}

/** A template as `literals[0] tokens[0] literals[1] tokens[1] ... literals[n]`. */
export interface TemplateModel {
  literals: string[];
  tokens: TemplateToken[];
}

const NAME = /^[A-Za-z_][\w.]*$/;
/** Words that are template keywords, never token names. */
const KEYWORDS = new Set(["end", "else", "endif", "endfor", "endforeach"]);

/** Index of the `}` that closes the `${` at `open`, or -1. Plain braces inside nest. */
function closingBrace(text: string, open: number): number {
  let depth = 1;
  for (let i = open + 2; i < text.length; i += 1) {
    const char = text[i];
    if (char === "{") depth += 1;
    else if (char === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Splits at commas outside parentheses and braces. */
function splitTopLevel(text: string): string[] {
  const parts: string[] = [];
  let depth = 0;
  let start = 0;
  for (let i = 0; i < text.length; i += 1) {
    const char = text[i];
    if (char === "(" || char === "{") depth += 1;
    else if (char === ")" || char === "}") depth = Math.max(0, depth - 1);
    else if (char === "," && depth === 0) {
      parts.push(text.slice(start, i));
      start = i + 1;
    }
  }
  parts.push(text.slice(start));
  return parts;
}

/** `title` from `title;first`, when the text is a token reference. */
function tokenName(text: string): string | null {
  const reference = text.trim();
  const semicolon = reference.indexOf(";");
  const name = semicolon === -1 ? reference : reference.slice(0, semicolon);
  if (!NAME.test(name) || KEYWORDS.has(name)) return null;
  return name;
}

/** Reads one `${...}` expression. */
export function classifyToken(source: string): TemplateToken {
  const inner = source.slice(2, -1);
  const advanced: TemplateToken = { source, kind: "advanced", name: "", prefix: "", suffix: "" };
  const parts = splitTopLevel(inner);
  if (parts.length === 1) {
    const name = tokenName(inner);
    return name === null ? advanced : { source, kind: "token", name, prefix: "", suffix: "" };
  }
  if (parts.length === 3) {
    const name = tokenName(parts[1] ?? "");
    if (name !== null) {
      return { source, kind: "optional", name, prefix: parts[0] ?? "", suffix: parts[2] ?? "" };
    }
  }
  return advanced;
}

/** Text to chips. Never fails: an unclosed `${` stays literal text. */
export function parseTemplate(text: string): TemplateModel {
  const literals: string[] = [];
  const tokens: TemplateToken[] = [];
  let literal = "";
  let i = 0;
  while (i < text.length) {
    if (text[i] === "$" && text[i + 1] === "{") {
      const close = closingBrace(text, i);
      if (close !== -1) {
        literals.push(literal);
        literal = "";
        tokens.push(classifyToken(text.slice(i, close + 1)));
        i = close + 1;
        continue;
      }
    }
    literal += text[i];
    i += 1;
  }
  literals.push(literal);
  return { literals, tokens };
}

/** Chips back to text. `serializeTemplate(parseTemplate(x)) === x` for every `x`. */
export function serializeTemplate(model: TemplateModel): string {
  let out = model.literals[0] ?? "";
  model.tokens.forEach((token, index) => {
    out += token.source + (model.literals[index + 1] ?? "");
  });
  return out;
}

/** The token for `${name}`. */
export function tokenFor(name: string): TemplateToken {
  return { source: `\${${name}}`, kind: "token", name, prefix: "", suffix: "" };
}

/** Replaces the literal at `index`. Typing a complete `${...}` there turns it into a chip. */
export function setLiteral(model: TemplateModel, index: number, text: string): TemplateModel {
  const literals = [...model.literals];
  literals[index] = text;
  const next = { literals, tokens: model.tokens };
  return text.includes("${") ? parseTemplate(serializeTemplate(next)) : next;
}

/** Puts a token into the literal at `index`, at `caret`, splitting the literal in two. */
export function insertToken(
  model: TemplateModel,
  index: number,
  caret: number,
  token: TemplateToken,
): TemplateModel {
  const literal = model.literals[index] ?? "";
  const at = Math.min(Math.max(caret, 0), literal.length);
  const literals = [...model.literals];
  literals.splice(index, 1, literal.slice(0, at), literal.slice(at));
  const tokens = [...model.tokens];
  tokens.splice(index, 0, token);
  return { literals, tokens };
}

/** Removes the token at `index`; the literals on both sides join. */
export function removeToken(model: TemplateModel, index: number): TemplateModel {
  const literals = [...model.literals];
  literals.splice(index, 2, (literals[index] ?? "") + (literals[index + 1] ?? ""));
  const tokens = [...model.tokens];
  tokens.splice(index, 1);
  return { literals, tokens };
}

/** What the `$` autocomplete is completing at the caret, if anything. */
export interface DollarQuery {
  /** Index of the `$` in the text. */
  start: number;
  /** The characters typed after it. */
  query: string;
}

/**
 * The `$` the caret is just after, with what was typed since. `${` is a finished token start and
 * does not count; neither does a `$` followed by anything but name characters.
 */
export function dollarQuery(text: string, caret: number): DollarQuery | null {
  let i = caret - 1;
  while (i >= 0 && /[\w.]/.test(text[i] ?? "")) i -= 1;
  if (i < 0 || text[i] !== "$") return null;
  if (text[i + 1] === "{") return null;
  return { start: i, query: text.slice(i + 1, caret) };
}

/** Tokens whose name or label starts with, then contains, the query; best first. */
export function matchTokens<T extends { name: string; label: string }>(
  tokens: readonly T[],
  query: string,
  limit = 8,
): T[] {
  const needle = query.toLowerCase();
  if (needle === "") return tokens.slice(0, limit);
  const starts: T[] = [];
  const contains: T[] = [];
  for (const token of tokens) {
    const name = token.name.toLowerCase();
    const label = token.label.toLowerCase();
    if (name.startsWith(needle) || label.startsWith(needle)) starts.push(token);
    else if (name.includes(needle) || label.includes(needle)) contains.push(token);
  }
  return [...starts, ...contains].slice(0, limit);
}
