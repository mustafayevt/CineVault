import { parseTemplate } from "../rename/template";
import type { TemplateToken } from "../rename/template";

/** The three templates the Naming section edits. */
export type NamingField = "movie_folder" | "movie_file" | "episode_file";

/** A token the template buttons insert: `{Title}` on the page, `${title}` in the engine's text. */
export interface NamingToken {
  label: string;
  engine: string;
}

/** The tokens of the movie templates, in the order of the buttons (spec `renamer`). */
export const MOVIE_TOKENS: readonly NamingToken[] = [
  { label: "Title", engine: "title" },
  { label: "Year", engine: "year" },
  { label: "Resolution", engine: "videoFormat" },
  { label: "HDR", engine: "hdrformat" },
  { label: "VideoCodec", engine: "videoCodec" },
  { label: "AudioCodec", engine: "audioCodec" },
  { label: "Channels", engine: "audioChannelsDot" },
  { label: "Edition", engine: "edition" },
  { label: "Source", engine: "mediaSource" },
  { label: "IMDb", engine: "imdb" },
  { label: "TMDB", engine: "tmdb" },
];

/** The tokens of the TV episode template. */
export const EPISODE_TOKENS: readonly NamingToken[] = [
  { label: "Show", engine: "showTitle" },
  { label: "ShowYear", engine: "showYear" },
  { label: "Season", engine: "seasonNr" },
  { label: "Season00", engine: "seasonNr2" },
  { label: "Episode", engine: "episodeNr" },
  { label: "Episode00", engine: "episodeNr2" },
  { label: "EpisodeTitle", engine: "title" },
  { label: "Resolution", engine: "videoFormat" },
  { label: "HDR", engine: "hdrformat" },
  { label: "VideoCodec", engine: "videoCodec" },
  { label: "AudioCodec", engine: "audioCodec" },
  { label: "Channels", engine: "audioChannelsDot" },
  { label: "Source", engine: "mediaSource" },
];

export function tokensOf(field: NamingField): readonly NamingToken[] {
  return field === "episode_file" ? EPISODE_TOKENS : MOVIE_TOKENS;
}

/**
 * A template as the field shows it: every plain `${name}` token that has a friendly name reads
 * `{Name}`; any other expression stays as the engine writes it. {@link toEngine} is its exact
 * inverse for templates written this way, so editing never changes a character the user did not
 * touch.
 */
export function toDisplay(engine: string, tokens: readonly NamingToken[]): string {
  const model = parseTemplate(engine);
  let out = model.literals[0] ?? "";
  model.tokens.forEach((token, index) => {
    const known = tokens.find((entry) => token.source === `\${${entry.engine}}`);
    out += known === undefined ? optionalDisplay(token, tokens) : `{${known.label}}`;
    out += model.literals[index + 1] ?? "";
  });
  return out;
}

/**
 * An optional token (`${ - ,edition,}`) as `{ - Edition?}`: the text around the name stays exactly
 * as written, the "?" says it disappears when the value is empty (the same form the Rename page
 * header shows). Anything that would not come back unchanged stays in the engine's syntax.
 */
function optionalDisplay(token: TemplateToken, tokens: readonly NamingToken[]): string {
  if (token.kind !== "optional") return token.source;
  const known = tokens.find((entry) => entry.engine === token.name);
  if (known === undefined) return token.source;
  const shown = `{${token.prefix}${known.label}?${token.suffix}}`;
  return toEngine(shown, tokens) === token.source ? shown : token.source;
}

/** `{prefix Label? suffix}` with a known label; the text around it has no braces or commas. */
function optionalPattern(tokens: readonly NamingToken[]): RegExp {
  const labels = tokens.map((entry) => entry.label).join("|");
  return new RegExp(String.raw`(\$?)\{([^{},]*?)(${labels})\?([^{},]*)\}`, "g");
}

/** The engine's text for what the field shows. */
export function toEngine(display: string, tokens: readonly NamingToken[]): string {
  const optional = display.replace(
    optionalPattern(tokens),
    (whole, dollar: string, prefix: string, label: string, suffix: string) => {
      const known = tokens.find((entry) => entry.label === label);
      return dollar !== "" || known === undefined
        ? whole
        : `\${${prefix},${known.engine},${suffix}}`;
    },
  );
  return optional.replace(/(\$?)\{([A-Za-z0-9]+)\}/g, (whole, dollar: string, name: string) => {
    if (dollar !== "") return whole;
    const known = tokens.find((entry) => entry.label === name);
    return known === undefined ? whole : `\${${known.engine}}`;
  });
}

/**
 * Why a template cannot be saved, in words, or `null`: a brace that is not closed or opened, or a
 * `{Name}` that is no token. Braces nest, since the engine's own expressions use them.
 */
export function displayProblem(display: string, tokens: readonly NamingToken[]): string | null {
  const open: number[] = [];
  for (let i = 0; i < display.length; i += 1) {
    const char = display[i];
    if (char === "{") open.push(i);
    else if (char === "}") {
      if (open.pop() === undefined)
        return `A closing brace at position ${i + 1} has no opening brace.`;
    }
  }
  const unclosed = open[open.length - 1];
  if (unclosed !== undefined) {
    const shown = /^\{[^\s{}]*/.exec(display.slice(unclosed))?.[0] ?? "{";
    return `The brace in "${shown}" is not closed. Add a } after the name.`;
  }
  for (const match of display.matchAll(/(\$?)\{([A-Za-z0-9]+)\}/g)) {
    if (match[1] === "" && !tokens.some((entry) => entry.label === match[2])) {
      return `{${match[2] ?? ""}} is not a token. Use the buttons under the field.`;
    }
  }
  return null;
}

/** Puts `{Label}` where the selection is and returns the text and where the caret goes. */
export function insertToken(
  text: string,
  start: number,
  end: number,
  label: string,
): { text: string; caret: number } {
  const piece = `{${label}}`;
  return { text: text.slice(0, start) + piece + text.slice(end), caret: start + piece.length };
}

/** How a colon in a title is written in names (spec `renamer` "Colon in titles option"). */
export type ColonStyle = "remove" | "dash" | "lookalike";

const COLON_STYLES: readonly ColonStyle[] = ["remove", "dash", "lookalike"];
export const COLON_SETTING_KEY = "naming.colon";
/** What the renamer writes until the user chooses: " - " (`Dune: Part Two` is `Dune - Part Two`). */
const COLON_DEFAULT: ColonStyle = "dash";

export function colonStyleOf(value: unknown): ColonStyle {
  return COLON_STYLES.find((style) => style === value) ?? COLON_DEFAULT;
}

/** `Dune: Part Two` as a name under the chosen colon style. */
export function withColonStyle(title: string, style: ColonStyle): string {
  switch (style) {
    case "remove":
      return title.replace(/:\s*/g, " ").replace(/\s{2,}/g, " ");
    case "dash":
      return title.replace(/:\s*/g, " - ");
    case "lookalike":
      return title.replace(/:/g, "꞉");
  }
}

/**
 * An example name from the backend with the colon of `title` written the chosen way. The plan
 * the example comes from was made before the choice changed, so it may hold the title written
 * any of the three ways; the example follows the choice at once.
 */
export function followColonStyle(name: string, title: string, style: ColonStyle): string {
  if (!title.includes(":")) return name;
  const chosen = withColonStyle(title, style);
  if (name.includes(chosen)) return name;
  const earlier = COLON_STYLES.map((other) => withColonStyle(title, other)).find((written) =>
    name.includes(written),
  );
  return earlier === undefined ? name : name.replace(earlier, chosen);
}
