import { existsSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

import { parseColor } from "../lib/contrast";
import { declarations, readDesignTokens, readStyle, themeColors } from "../test/tokenCss";

const tokens = readDesignTokens();
const css = readStyle("tokens.css");

function normalise(value: string): string {
  return value.replace(/\s+/g, "").toLowerCase();
}

function sameColor(actual: string, expected: string): boolean {
  if (normalise(actual) === normalise(expected)) return true;
  return JSON.stringify(parseColor(actual)) === JSON.stringify(parseColor(expected));
}

describe("tokens.css matches the design system tokens.json", () => {
  for (const theme of ["dark", "light"] as const) {
    it(`defines every ${theme} colour token with the exact value`, () => {
      const colors = themeColors(theme);
      for (const token of tokens.color.tokens) {
        expect(colors[token.name], `${theme} ${token.name}`).toBeDefined();
        expect(
          sameColor(colors[token.name] ?? "", token.value[theme]),
          `${theme} ${token.name}`,
        ).toBe(true);
      }
    });
  }

  for (const theme of ["dark", "light"] as const) {
    it(`defines the ${theme} floating shadow with the exact value`, () => {
      const values = themeColors(theme);
      for (const token of tokens.shadow.tokens) {
        expect(normalise(values[token.name] ?? ""), `${theme} ${token.name}`).toBe(
          normalise(token.value[theme]),
        );
      }
    });
  }

  it("defines spacing, radius, type, size and duration variables on :root", () => {
    const root = declarations(css, ":root");
    for (const group of [
      tokens.spacing,
      tokens.radius,
      tokens.fontSize,
      tokens.fontWeight,
      tokens.size,
      tokens.duration,
      tokens.motion,
    ]) {
      for (const token of group.tokens) {
        expect(root.get(`--${token.name}`), token.name).toBe(token.value);
      }
    }
  });

  it("uses the radius scale 4 / 6 / 10 / 14 / 999 and the type scale 11-28", () => {
    expect(tokens.radius.tokens.map((t) => t.value)).toEqual([
      "4px",
      "6px",
      "10px",
      "14px",
      "999px",
    ]);
    expect(tokens.fontSize.tokens.map((t) => t.value)).toEqual([
      "11px",
      "12px",
      "13px",
      "14px",
      "16px",
      "20px",
      "28px",
    ]);
    expect(tokens.fontWeight.tokens.map((t) => t.value)).toEqual(["500", "600", "700"]);
  });

  it("emits easing variables even when no utility uses them", () => {
    expect(css).toContain("@theme static {");
    const theme = declarations(css, "@theme static");
    for (const token of tokens.easing.tokens) {
      expect(normalise(theme.get(`--${token.name}`) ?? ""), token.name).toBe(
        normalise(token.value),
      );
    }
  });

  it("keeps the motion tokens: no duration above dur-max, enter ease-out and exit ease-in", () => {
    const durations = Object.fromEntries(
      tokens.duration.tokens.map((t) => [t.name, parseFloat(t.value)]),
    );
    expect(Object.keys(durations).sort()).toEqual(
      ["dur-bar", "dur-expand", "dur-hover", "dur-max", "dur-page", "dur-pop"].sort(),
    );
    for (const [name, ms] of Object.entries(durations)) {
      expect(ms, name).toBeLessThanOrEqual(durations["dur-max"] ?? 0);
    }
    expect(tokens.easing.tokens.map((t) => t.name)).toEqual(["ease-out", "ease-in"]);
    expect(tokens.motion.tokens.map((t) => t.name)).toEqual(["pop-scale", "lift"]);
  });

  it("uses two font families and no display serif", () => {
    const theme = declarations(css, "@theme static");
    expect(theme.get("--font-display")).toBeUndefined();
    expect(Object.keys(tokens.type.families).sort()).toEqual(["mono", "sans"]);
    expect(theme.get("--font-sans")).toBe(tokens.type.families["sans"]);
    expect(theme.get("--font-mono")).toBe(tokens.type.families["mono"]);
  });

  it("scopes themes by any [data-theme] element so a panel can show the other theme", () => {
    expect(css).toContain('[data-theme="dark"] {');
    expect(css).toContain('[data-theme="light"] {');
    expect(css).not.toContain(':root[data-theme="light"]');
  });
});

describe("type styles", () => {
  const typography = readStyle("typography.css");
  /** The token a size or weight in the type table stands for: `13px` is `var(--fs-13)`. */
  const sizeToken = (size: string) => `var(--fs-${parseInt(size, 10)})`;
  const weightToken = (weight: number) => `var(--fw-${weight})`;

  it("uses only sizes and weights that are tokens", () => {
    const sizes = tokens.fontSize.tokens.map((t) => t.value);
    const weights = tokens.fontWeight.tokens.map((t) => Number(t.value));
    for (const group of tokens.type.groups) {
      for (const style of group.styles) {
        expect(sizes, `${style.name} size`).toContain(style.fontSize);
        expect(weights, `${style.name} weight`).toContain(style.fontWeight);
      }
    }
  });

  for (const group of tokens.type.groups) {
    for (const style of group.styles) {
      it(`has .type-${style.name} with the token size and weight`, () => {
        const start = typography.search(
          new RegExp(String.raw`^\s*\.type-${style.name}[,\s{]`, "m"),
        );
        expect(start, style.name).toBeGreaterThanOrEqual(0);
        const open = typography.indexOf("{", start);
        const rule = typography.slice(open, typography.indexOf("}", open));
        const line =
          typeof style.lineHeight === "number" ? String(style.lineHeight) : style.lineHeight;
        expect(rule).toContain(
          `font: ${weightToken(style.fontWeight)} ${sizeToken(style.fontSize)}/${line} var(--font-${group.family})`,
        );
        if (style.letterSpacing) expect(rule).toContain(`letter-spacing: ${style.letterSpacing}`);
      });
    }
  }
});

describe("bundled fonts", () => {
  const fonts = readStyle("fonts.css");

  it("declares Archivo and JetBrains Mono, and no serif", () => {
    for (const family of ["Archivo", "JetBrains Mono"]) {
      expect(fonts).toMatch(new RegExp(`font-family: ["']${family}["'];`));
    }
    expect(fonts).not.toMatch(/serif/i);
  });

  it("gives Archivo its width axis so headings can run wider", () => {
    expect(fonts).toContain("font-stretch: 62% 125%;");
  });

  it("loads only local files, never the network", () => {
    expect(fonts).not.toMatch(/https?:|\/\//);
    const urls = [...fonts.matchAll(/url\("([^"]+)"\)/g)].map((m) => m[1] ?? "");
    expect(urls.length).toBeGreaterThan(0);
    for (const url of urls) {
      expect(existsSync(resolve(__dirname, url)), url).toBe(true);
    }
  });

  it("swaps fonts in without blocking text", () => {
    const faces = fonts.split("@font-face").slice(1);
    for (const face of faces) expect(face).toContain("font-display: swap");
  });
});

describe("tokens.css themes", () => {
  it("declares the same colour tokens in both themes", () => {
    expect(Object.keys(themeColors("light")).sort()).toEqual(
      Object.keys(themeColors("dark")).sort(),
    );
  });
});
