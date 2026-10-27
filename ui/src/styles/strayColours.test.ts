import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * The design tokens are the only source of colour: `tokens.css` holds every hex, rgb and hsl value.
 * Everything else reads `var(--token)` or a Tailwind utility that maps one. The listed files are
 * the documented exceptions: rating-source brand colours, and data (not styling) that merely looks
 * like a colour.
 */
const ALLOWED = new Set([
  "styles/tokens.css",
  // Rating-source brand colours (IMDb yellow, Rotten Tomatoes red, ...).
  "features/ratings/ratingSources.ts",
  // The rating sources' logos, drawn in their brand colours.
  "features/ratings/ratingMarks.tsx",
  // Placeholder pictures drawn for the synthetic backend: image data, not styling.
  "features/images/syntheticImages.ts",
  // Generated typographic posters, backdrops, stills and faces: picture data, not interface colour.
  "components/posterArt.ts",
  "components/PlaceholderPoster.tsx",
]);

// A hex ends at the first character that is not a letter or digit: `#000_calc(...)` in a Tailwind
// arbitrary value counts too (`\b` would stop at the underscore and miss it).
const COLOUR = /#[0-9a-fA-F]{3,8}(?![0-9a-zA-Z])|\b(?:rgba?|hsla?)\(/;

/** Findings in one source text: the line numbers that hold a colour literal. */
export function strayColourLines(source: string): number[] {
  const lines: number[] = [];
  source.split("\n").forEach((line, index) => {
    // Comments may quote a colour while explaining a token.
    const code = line.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
    if (/^\s*(\*|\/\*)/.test(line)) return;
    if (COLOUR.test(code)) lines.push(index + 1);
  });
  return lines;
}

function sourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "test") files.push(...sourceFiles(path));
    } else if (
      /\.(css|tsx?)$/.test(entry.name) &&
      !/\.test\.tsx?$/.test(entry.name) &&
      entry.name !== "bindings.ts"
    ) {
      files.push(path);
    }
  }
  return files;
}

describe("stray colours", () => {
  it("flags a hex, rgb or hsl literal and ignores tokens and comments", () => {
    expect(strayColourLines("a { color: #fff; }")).toEqual([1]);
    expect(strayColourLines("a { color: rgb(0 0 0); }\nb { color: hsl(1 2% 3%); }")).toEqual([
      1, 2,
    ]);
    expect(strayColourLines("a { color: var(--text); }")).toEqual([]);
    expect(
      strayColourLines('"[mask-image:linear-gradient(to_right,#000_calc(100%-24px),x)]"'),
    ).toEqual([1]);
    expect(strayColourLines("/* #fff */\n// #123456\n * rgba(0,0,0)")).toEqual([]);
  });

  it("finds no colour literal in the UI source outside the token file and the brand colours", () => {
    const root = resolve(__dirname, "..");
    const found: string[] = [];
    for (const file of sourceFiles(root)) {
      const name = relative(root, file).replace(/\\/g, "/");
      if (ALLOWED.has(name)) continue;
      for (const line of strayColourLines(readFileSync(file, "utf8")))
        found.push(`${name}:${line}`);
    }
    expect(found).toEqual([]);
  });
});
