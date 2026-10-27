import { readdirSync, readFileSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { readStyle } from "../test/tokenCss";

const full = readStyle("motion.css");
const MARKER = "==== Small effects and reduced motion ====";
/** The motion primitives (tokens only) and, after the marker, the small effects and reduced-motion rules. */
const desktop = full.slice(0, full.indexOf(`/* ${MARKER}`));
const css = full.slice(full.indexOf(MARKER));
const tokenCss = readStyle("tokens.css");

/** The text of the first block that starts with `header`, braces balanced. */
function block(source: string, header: string): string {
  const start = source.indexOf(header);
  expect(start, `${header} is missing`).toBeGreaterThanOrEqual(0);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    if (source[i] === "{") depth++;
    if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  throw new Error(`${header} is not closed`);
}

/** The class names defined by a stylesheet, in order of first appearance. */
function classNames(source: string): string[] {
  return [...new Set([...source.matchAll(/^\.(cv-[\w-]+)/gm)].map((m) => m[1] ?? ""))];
}

describe("desktop motion primitives", () => {
  const classes = ["hoverable", "pop", "slide-up", "page", "expand", "lift", "bar", "xfade"];

  it("has the primitives before the marked small-effects section", () => {
    expect(full).toContain(MARKER);
    expect(desktop.length).toBeGreaterThan(1000);
  });

  it("defines every primitive class", () => {
    expect(classNames(desktop)).toEqual(classes.map((c) => `cv-${c}`));
  });

  it("takes every duration from a --dur-* token", () => {
    const code = desktop.replace(/\/\*[\s\S]*?\*\//g, "");
    const literals = [...code.matchAll(/(?<![\w.-])(\d*\.?\d+)(ms|s)(?![\w-])/g)]
      .map((m) => m[0])
      .filter((value) => parseFloat(value) !== 0);
    expect(literals).toEqual([]);
    const used = new Set([...code.matchAll(/var\((--dur-[\w-]+)\)/g)].map((m) => m[1]));
    const declared = new Set([...tokenCss.matchAll(/(--dur-[\w-]+):/g)].map((m) => m[1]));
    for (const name of used) expect(declared.has(name), `${name} is not a token`).toBe(true);
    expect([...used].sort()).toEqual([
      "--dur-bar",
      "--dur-expand",
      "--dur-hover",
      "--dur-page",
      "--dur-pop",
    ]);
  });

  it("takes every easing from --ease-out or --ease-in, never a literal curve", () => {
    const code = desktop.replace(/\/\*[\s\S]*?\*\//g, "");
    expect(code).not.toMatch(/cubic-bezier|steps\(|(?<![\w-])(ease|linear|ease-in-out)(?![\w-])/);
    expect(code).toMatch(/var\(--ease-out\)/);
    expect(code).toMatch(/var\(--ease-in\)/);
  });

  it("keeps every duration token at or below --dur-max", () => {
    const value = (name: string) =>
      parseFloat(new RegExp(String.raw`${name}:\s*([\d.]+)ms`).exec(tokenCss)?.[1] ?? "NaN");
    const max = value("--dur-max");
    expect(max).toBe(250);
    for (const name of ["hover", "pop", "page", "bar", "expand"]) {
      expect(value(`--dur-${name}`), name).toBeLessThanOrEqual(max);
    }
  });

  it("starts menus and popovers from --pop-scale and grows them with the enter and exit curves", () => {
    expect(block(desktop, "@keyframes cv-pop-in")).toContain("scale(var(--pop-scale))");
    expect(block(desktop, ".cv-pop {")).toContain(
      "animation: cv-pop-in var(--dur-pop) var(--ease-out)",
    );
    expect(block(desktop, '.cv-pop[data-state="closed"] {')).toContain("var(--ease-in)");
  });

  it("grows expanding rows through the grid track, 0fr to 1fr", () => {
    expect(block(desktop, ".cv-expand {")).toContain("grid-template-rows: 0fr");
    expect(block(desktop, ".cv-expand:is([data-open")).toContain("grid-template-rows: 1fr");
  });

  it("lifts posters by the lift token and scales bars by transform", () => {
    expect(block(desktop, ".cv-lift:hover {")).toContain("translateY(var(--lift))");
    const bar = block(desktop, ".cv-bar {");
    expect(bar).toContain("transform: scaleX(var(--progress, 0))");
    expect(bar).toMatch(/transition:\s*transform var\(--dur-bar\)/);
  });

  it("animates only transform, opacity, filter, colours and the expand track", () => {
    expect(cssMotionProblems(desktop)).toEqual([]);
  });

  describe("reduced motion", () => {
    const start = desktop.indexOf(':root[data-motion="reduced"] .cv-hoverable');
    const reducedAttr = desktop.slice(start, desktop.indexOf("@media (prefers-reduced-motion"));
    const reducedMedia = block(desktop, "@media (prefers-reduced-motion: reduce)");

    for (const name of classes) {
      it(`has a rule for .cv-${name} for the setting and for the system preference`, () => {
        expect(reducedAttr, `data-motion="reduced" .cv-${name}`).toMatch(
          new RegExp(String.raw`\.cv-${name}(?![\w-])`),
        );
        expect(reducedMedia, `media .cv-${name}`).toMatch(
          new RegExp(String.raw`\.cv-${name}(?![\w-])`),
        );
      });
    }

    it("leaves no scaling, sliding or lifting: pop and slide fall back to a fade", () => {
      for (const rules of [reducedAttr, reducedMedia]) {
        expect(rules).toContain("animation-name: cv-fade-in");
        expect(rules).toContain("transform: none");
        expect(rules).toContain("transition: none");
      }
    });
  });
});

describe("small effects and reduced motion", () => {
  it("is imported by the app stylesheet", () => {
    expect(readStyle("index.css")).toContain('@import "./motion.css"');
  });

  it("offers the hover duration from the tokens with the out easing", () => {
    const rule = block(css, ".motion-fast {");
    expect(rule).toContain("transition-duration: var(--dur-hover)");
    expect(rule).toContain("transition-timing-function: var(--ease-out)");
  });

  it("turns motion into a 100 ms crossfade for the setting and for the system preference", () => {
    for (const header of [
      ':root[data-motion="reduced"] *,',
      "@media (prefers-reduced-motion: reduce)",
    ]) {
      const rules = header.startsWith("@") ? block(css, header) : css;
      expect(rules).toContain("animation-duration: 100ms !important");
      expect(rules).toContain("transition-duration: 100ms !important");
    }
    // Only fades remain: no transform or size transitions.
    expect(css).toMatch(/transition-property:\s*[^;]*opacity/);
    expect(css).not.toMatch(/transition-property:[^;]*transform/);
    expect(css).not.toContain("0.01ms");
    expect(css).toMatch(/--motion-shift:\s*0px/);
  });

  it("lets the Motion setting force full motion over the system preference", () => {
    expect(block(css, "@media (prefers-reduced-motion: reduce)")).toContain(
      ':root:not([data-motion="full"])',
    );
  });
});

/** Properties whose animation forces layout; only transform and opacity may move. */
const LAYOUT = String.raw`(?:min-|max-)?(?:width|height)|top|left|right|bottom|inset|margin|padding|grid-template(?:-columns|-areas)?|flex-basis|all`;
const LAYOUT_WORD = new RegExp(String.raw`(?<![\w-])(?:${LAYOUT})(?![\w-])`);

/** Problems in a stylesheet: layout properties in transitions or in keyframes. */
export function cssMotionProblems(source: string): string[] {
  const problems: string[] = [];
  for (const match of source.matchAll(/(?<![\w-])transition(?:-property)?\s*:([^;}]*)[;}]/g)) {
    const value = match[1] ?? "";
    // Strip var(...) and timing words so "all" or "top" inside a token name is not misread.
    const properties = value.replace(/var\([^)]*\)/g, "").replace(/cubic-bezier\([^)]*\)/g, "");
    if (LAYOUT_WORD.test(properties) && !/^\s*none\s*$/.test(properties)) {
      problems.push(`transition of a layout property: ${match[0].trim()}`);
    }
  }
  for (const frames of source.matchAll(/@keyframes\s+([\w-]+)\s*\{/g)) {
    const body = block(source.slice(frames.index), "{");
    for (const declaration of body.matchAll(/(?<![\w-])([a-z-]+)\s*:/g)) {
      if (LAYOUT_WORD.test(declaration[1] ?? "")) {
        problems.push(`keyframes ${frames[1]} animates ${declaration[1]}`);
      }
    }
  }
  return problems;
}

/** Problems in component source: Tailwind transition classes and inline styles on layout properties. */
export function sourceMotionProblems(source: string): string[] {
  const problems: string[] = [];
  for (const match of source.matchAll(/transition-\[([^\]]*)\]/g)) {
    if (LAYOUT_WORD.test(match[1] ?? "")) problems.push(`class ${match[0]}`);
  }
  for (const match of source.matchAll(
    /(?<![\w-])transition-(all|width|height|top|left|right|bottom|inset|margin|padding|\(?grid)/g,
  )) {
    problems.push(`class ${match[0]}`);
  }
  for (const match of source.matchAll(
    /(?<![\w-])transition(?:Property)?\s*:\s*["'`]([^"'`]*)["'`]/g,
  )) {
    if (LAYOUT_WORD.test(match[1] ?? "")) problems.push(`inline style ${match[0]}`);
  }
  return problems;
}

function sourceFiles(dir: string): string[] {
  const files: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const path = join(dir, entry.name);
    if (entry.isDirectory()) {
      if (entry.name !== "node_modules" && entry.name !== "test") files.push(...sourceFiles(path));
    } else if (/\.(css|tsx?)$/.test(entry.name) && !/\.test\.tsx?$/.test(entry.name)) {
      files.push(path);
    }
  }
  return files;
}

describe("motion lint", () => {
  it("flags transitions and animations of layout properties", () => {
    expect(cssMotionProblems(".a { transition: width 180ms ease; }")).toHaveLength(1);
    expect(cssMotionProblems(".a { transition: opacity 1s, height 1s; }")).toHaveLength(1);
    expect(cssMotionProblems(".a { transition-property: top, left; }")).toHaveLength(1);
    expect(cssMotionProblems(".a { transition: all 1s; }")).toHaveLength(1);
    expect(cssMotionProblems(".a { transition: grid-template-columns 1s; }")).toHaveLength(1);
    expect(
      cssMotionProblems("@keyframes grow { from { width: 0; } to { width: 100%; } }"),
    ).not.toHaveLength(0);
    expect(
      cssMotionProblems("@keyframes up { from { top: 4px; } to { top: 0; } }"),
    ).not.toHaveLength(0);
    expect(sourceMotionProblems('className="transition-[width] duration-base"')).toHaveLength(1);
    expect(sourceMotionProblems('className="transition-all"')).toHaveLength(1);
    expect(sourceMotionProblems('style={{ transition: "height 1s" }}')).toHaveLength(1);
  });

  it("allows transform and opacity", () => {
    expect(
      cssMotionProblems(".a { transition: transform var(--dur-bar) var(--ease-out), opacity 1s; }"),
    ).toEqual([]);
    expect(
      cssMotionProblems(
        "@keyframes in { from { opacity: 0; transform: translateX(12px); } to { opacity: 1; } }",
      ),
    ).toEqual([]);
    expect(
      sourceMotionProblems('className="transition-[background-color,filter] transition-transform"'),
    ).toEqual([]);
  });

  it("finds no transition or animation of width, height, top, left or grid-template in the UI source", () => {
    const root = resolve(__dirname, "..");
    const problems: string[] = [];
    for (const file of sourceFiles(root)) {
      const text = readFileSync(file, "utf8");
      const found = file.endsWith(".css") ? cssMotionProblems(text) : sourceMotionProblems(text);
      for (const problem of found) problems.push(`${relative(root, file)}: ${problem}`);
    }
    expect(problems).toEqual([]);
  });
});
