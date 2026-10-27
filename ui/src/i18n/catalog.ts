import { activeLocale, numberFormat } from "./locale";
import { artwork } from "./messages/en/artwork";
import { common } from "./messages/en/common";
import { deleteMessages } from "./messages/en/delete";
import { drives } from "./messages/en/drives";
import { history } from "./messages/en/history";
import { library } from "./messages/en/library";
import { movie } from "./messages/en/movie";
import { onboarding } from "./messages/en/onboarding";
import { rename } from "./messages/en/rename";
import { scrape } from "./messages/en/scrape";
import { settings } from "./messages/en/settings";
import { shell } from "./messages/en/shell";
import { show } from "./messages/en/show";
import { torrents } from "./messages/en/torrents";

/**
 * The English string catalogue, one file per area under `messages/en/` so two page groups never
 * edit the same file. Every piece of user-facing text is keyed by a stable id.
 *
 * Rules for messages:
 * - `{name}` inserts a parameter. A number parameter is formatted for the selected locale.
 * - `{count, plural, one {# title} other {# titles}}` picks a branch by plural category; `#` stands
 *   for the (locale-formatted) number. `=0 {...}` matches an exact value. Always give `other`.
 * - Settings search keywords (`*.keywords`) are comma-separated lists.
 * - Proper names (CineVault, Kodi, Jellyfin, Emby) are written literally in code, not here.
 * - A key lives in exactly one area file (the test checks it).
 */
export const EN_AREAS = {
  shell,
  library,
  movie,
  show,
  scrape,
  artwork,
  rename,
  drives,
  torrents,
  history,
  settings,
  onboarding,
  delete: deleteMessages,
  common,
} as const;

const en = {
  ...shell,
  ...library,
  ...movie,
  ...show,
  ...scrape,
  ...artwork,
  ...rename,
  ...drives,
  ...torrents,
  ...history,
  ...settings,
  ...onboarding,
  ...deleteMessages,
  ...common,
} as const;

export type MessageKey = keyof typeof en;

/** Values a message can insert. Numbers are formatted for the selected locale. */
export type MessageParams = Readonly<Record<string, string | number>>;

/**
 * The language of the catalogue in use. English is the only one for now; plural categories are
 * chosen by this language, not by the regional format, so "1 job" stays correct whatever the
 * date and number region is.
 */
const CATALOGUE_LANGUAGE = "en";

const messages: Readonly<Record<MessageKey, string>> = en;

const pluralRules = new Intl.PluralRules(CATALOGUE_LANGUAGE);

function formatParam(value: string | number): string {
  return typeof value === "number" ? numberFormat(activeLocale()).format(value) : value;
}

/** Index of the `}` that closes the `{` at `open`, or -1 when it never closes. */
function closingBrace(text: string, open: number): number {
  let depth = 0;
  for (let i = open; i < text.length; i += 1) {
    if (text[i] === "{") depth += 1;
    else if (text[i] === "}") {
      depth -= 1;
      if (depth === 0) return i;
    }
  }
  return -1;
}

/** Parses `one {# job} other {# jobs}` into selector and text pairs. */
function parseBranches(text: string): Map<string, string> {
  const branches = new Map<string, string>();
  let i = 0;
  while (i < text.length) {
    while (i < text.length && /\s/.test(text[i] ?? "")) i += 1;
    const start = i;
    while (i < text.length && text[i] !== "{" && !/\s/.test(text[i] ?? "")) i += 1;
    const selector = text.slice(start, i);
    while (i < text.length && /\s/.test(text[i] ?? "")) i += 1;
    if (selector === "" || text[i] !== "{") break;
    const close = closingBrace(text, i);
    if (close === -1) break;
    branches.set(selector, text.slice(i + 1, close));
    i = close + 1;
  }
  return branches;
}

function expandPlural(count: number, branchText: string, params: MessageParams): string {
  const branches = parseBranches(branchText);
  const chosen =
    branches.get(`=${count}`) ??
    branches.get(pluralRules.select(count)) ??
    branches.get("other") ??
    "";
  return render(chosen.replaceAll("#", formatParam(count)), params);
}

function expand(body: string, params: MessageParams): string {
  const comma = body.indexOf(",");
  const name = (comma === -1 ? body : body.slice(0, comma)).trim();
  const value = params[name];
  if (comma === -1) return value === undefined ? `{${name}}` : formatParam(value);
  const rest = body.slice(comma + 1).trimStart();
  if (rest.startsWith("plural,") && typeof value === "number") {
    return expandPlural(value, rest.slice("plural,".length), params);
  }
  return `{${body}}`;
}

function render(template: string, params: MessageParams): string {
  let out = "";
  let i = 0;
  while (i < template.length) {
    if (template[i] === "{") {
      const close = closingBrace(template, i);
      if (close === -1) return out + template.slice(i);
      out += expand(template.slice(i + 1, close), params);
      i = close + 1;
    } else {
      out += template[i];
      i += 1;
    }
  }
  return out;
}

/** The message for `key` with its parameters inserted. */
export function t(key: MessageKey, params: MessageParams = {}): string {
  const template = messages[key] as string | undefined;
  return render(template ?? key, params);
}

/** A comma-separated message (search keywords) as a list. */
export function tList(key: MessageKey): readonly string[] {
  return t(key)
    .split(",")
    .map((word) => word.trim())
    .filter((word) => word !== "");
}

/** Whether `key` names a message in the catalogue. */
export function hasMessage(key: string): key is MessageKey {
  return Object.hasOwn(messages, key);
}

/** Every key and its raw template, for tests and tooling. */
export function allMessages(): Readonly<Record<MessageKey, string>> {
  return messages;
}
