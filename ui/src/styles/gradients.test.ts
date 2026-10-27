import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * No decorative gradients: surfaces, buttons, chips, skeletons and generated pictures are flat
 * tones from the tokens. The only gradients are readability scrims over real artwork, each marked
 * with the word SCRIM in the comment just above it, in the files listed here.
 */
const SCRIM_FILES = new Set(["features/library/library.css", "features/title/title.css"]);

const GRADIENT =
  /(?:linear|radial|conic)-gradient\(|<\s*(?:linear|radial)Gradient|\bbg-(?:linear|radial|conic)|\b(?:from|via|to)-\[|\bgradientUnits\b/;

/** The line numbers (1-based) of gradient code in `source`, comments left out. */
export function gradientLines(source: string): number[] {
  const lines: number[] = [];
  source.split("\n").forEach((line, index) => {
    if (/^\s*(\*|\/\*|\/\/)/.test(line)) return;
    const code = line.replace(/\/\/.*$/, "").replace(/\/\*.*?\*\//g, "");
    if (GRADIENT.test(code)) lines.push(index + 1);
  });
  return lines;
}

/** Whether a scrim comment sits within a few lines above `line`. */
function isMarkedScrim(source: string, line: number): boolean {
  const above = source.split("\n").slice(Math.max(0, line - 9), line - 1);
  return above.some((text) => /SCRIM/.test(text));
}

function sourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "test") files.push(...sourceFiles(path));
    } else if (
      /\.(css|tsx?|svg|json)$/.test(entry.name) &&
      !/\.test\.tsx?$/.test(entry.name) &&
      entry.name !== "bindings.ts"
    ) {
      files.push(path);
    }
  }
  return files;
}

describe("gradients", () => {
  it("finds gradient code and ignores comments", () => {
    expect(gradientLines("a { background: linear-gradient(red, blue); }")).toEqual([1]);
    expect(gradientLines('<radialGradient id="x">')).toEqual([1]);
    expect(gradientLines('class="bg-linear-to-t from-[#000]"')).toEqual([1]);
    expect(gradientLines("/* no linear-gradient( here */\n// radial-gradient(\nplain")).toEqual([]);
  });

  it("allows only the marked scrims over artwork", () => {
    const root = resolve(__dirname, "..");
    const stray: string[] = [];
    for (const file of sourceFiles(root)) {
      const name = relative(root, file).replaceAll("\\", "/");
      const source = readFileSync(file, "utf8");
      for (const line of gradientLines(source)) {
        if (!(SCRIM_FILES.has(name) && isMarkedScrim(source, line))) stray.push(`${name}:${line}`);
      }
    }
    expect(stray, "decorative gradient: use a flat token surface, or mark a scrim").toEqual([]);
  });
});
