import { hasMessage, t } from "../../i18n/catalog";
import { parseTemplate } from "./template";
import type { TemplateToken } from "./template";

/**
 * The friendly name of a token (`videoFormat` is "Resolution"). A token the catalogue has no name
 * for yet is spelled out from its parts (`audioCodecList` is "Audio codec list"), so a raw
 * internal name never shows.
 */
export function tokenLabel(name: string): string {
  const key = `organize.token.${name}`;
  return hasMessage(key) ? t(key) : spelledOut(name);
}

/** `audioCodecList` as "Audio codec list", `seasonNr2` as "Season nr 2". */
function spelledOut(name: string): string {
  const words = name
    .replace(/([a-z])([A-Z0-9])/g, "$1 $2")
    .replace(/([0-9])([A-Za-z])/g, "$1 $2")
    .toLowerCase()
    .trim();
  return words === "" ? name : words.charAt(0).toUpperCase() + words.slice(1);
}

/** What a chip says: the label, with an optional token's text around it and a "?" after it. */
export function chipText(token: TemplateToken): string {
  switch (token.kind) {
    case "token":
      return tokenLabel(token.name);
    case "optional": {
      const before = token.prefix.trim();
      const after = token.suffix.trim();
      return `${before === "" ? "" : `${before} `}${tokenLabel(token.name)}${
        after === "" ? "" : ` ${after}`
      }?`;
    }
    case "advanced":
      return token.source.slice(2, -1).trim();
  }
}

/**
 * A template as the page shows it in one line: every token by its friendly name in braces
 * (`${title} (${year})` reads `{Title} ({Year})`), an optional token with the text around it and
 * a "?" (`${title}${ - ,edition,}` reads `{Title}{ - Edition?}`), and any other expression as written. The chips and the raw editor keep the engine syntax.
 */
export function displayTemplate(text: string): string {
  const model = parseTemplate(text);
  let out = model.literals[0] ?? "";
  model.tokens.forEach((token, index) => {
    const shown =
      token.kind === "advanced"
        ? token.source
        : token.kind === "optional"
          ? // Exact text around the name, so the Naming field reads and edits the same form.
            `{${token.prefix}${tokenLabel(token.name)}?${token.suffix}}`
          : `{${tokenLabel(token.name)}}`;
    out += shown + (model.literals[index + 1] ?? "");
  });
  return out;
}
