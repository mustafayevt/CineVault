import { describe, expect, it } from "vitest";

import { composite, contrastRatio, parseColor } from "../lib/contrast";
import type { Rgba } from "../lib/contrast";
import { themeColors } from "../test/tokenCss";
import { CONTRAST_PAIRS, KNOWN_TOKEN_SHORTFALLS } from "./contrastPairs";
import type { ContrastPair } from "./contrastPairs";

const themes = ["dark", "light"] as const;

function resolveToken(colors: Record<string, string>, name: string, depth = 0): Rgba {
  const value = colors[name];
  if (value === undefined) throw new Error(`Unknown colour token: ${name}`);
  const alias = /^var\(--([\w-]+)\)$/.exec(value);
  if (alias?.[1]) {
    if (depth > 4) throw new Error(`Alias loop at ${name}`);
    return resolveToken(colors, alias[1], depth + 1);
  }
  return parseColor(value);
}

function ratioFor(pair: ContrastPair, theme: (typeof themes)[number]): number {
  const colors = themeColors(theme);
  let ground = resolveToken(colors, pair.ground);
  // Scrim sits on artwork; the worst case is a white poster underneath.
  if (pair.ground === "scrim") ground = composite(ground, { r: 255, g: 255, b: 255, a: 1 });
  if (pair.soft) ground = composite(resolveToken(colors, pair.soft), ground);
  const fg = resolveToken(colors, pair.fg);
  const text = composite({ ...fg, a: fg.a * (pair.fgOpacity ?? 1) }, ground);
  return contrastRatio(text, ground);
}

describe("WCAG 2.1 AA contrast of the design tokens", () => {
  for (const theme of themes) {
    describe(`${theme} theme`, () => {
      for (const pair of CONTRAST_PAIRS) {
        it(`${pair.usage} reaches ${pair.min}:1`, () => {
          const ratio = ratioFor(pair, theme);
          expect(
            ratio,
            `${pair.fg} on ${pair.soft ?? pair.ground}: ${ratio.toFixed(2)}`,
          ).toBeGreaterThanOrEqual(pair.min);
        });
      }
    });
  }
});

describe("known token shortfalls stay recorded", () => {
  it("lists only real shortfalls", () => {
    expect(Array.isArray(KNOWN_TOKEN_SHORTFALLS)).toBe(true);
  });
  for (const entry of KNOWN_TOKEN_SHORTFALLS) {
    for (const theme of entry.themes) {
      it(`${entry.usage} (${theme}) is still below ${entry.min}:1`, () => {
        // If this fails the tokens were improved: delete the entry from KNOWN_TOKEN_SHORTFALLS.
        expect(ratioFor(entry, theme)).toBeLessThan(entry.min);
      });
    }
  }
});
