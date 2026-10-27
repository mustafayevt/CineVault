import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";

import { describe, expect, it } from "vitest";

const SRC = resolve(__dirname, "..");

/** The token names of the old design system, each with the token that replaced it. */
const COLOUR_NAMES = [
  "bg",
  "bg-titlebar",
  "bg-sidebar",
  "surface",
  "surface-selected",
  "surface-hi",
  "surface-sunken",
  "line",
  "line-faint",
  "line-strong",
  "text-secondary",
  "primary",
  "primary-hover",
  "on-primary",
  "accent",
  "accent-text",
  "accent-hover",
  "accent-soft",
  "hl",
  "on-accent",
  "success",
  "success-text",
  "success-soft",
  "warning",
  "warning-text",
  "warning-soft",
  "danger-text",
  "danger-soft",
  "info",
  "info-text",
  "info-soft",
];
const VARIABLE_NAMES = [
  ...COLOUR_NAMES,
  "shadow-ink",
  "shadow-edge",
  "titlebar-height",
  "statusbar-height",
  "sidebar-width",
  "rail-width",
  "inspector-width",
  "dock-width",
  "drawer-width",
  "control-height",
  "control-height-lg",
  "control-bg",
  "row-h",
  "row-h-compact",
  "poster-min",
  "radius-xs",
  "radius-sm",
  "radius-md",
  "radius-lg",
  "radius-xl",
  "radius-pill",
  "shadow-poster",
  "shadow-poster-hover",
  "shadow-thumb",
  "duration-fast",
  "duration-base",
  "duration-slow",
  "ease-shutter",
  "ease-settle",
];
const TYPE_NAMES = [
  "display-hero",
  "display-onboarding",
  "display-page",
  "display-poster",
  "display-section",
  "stat",
  "body-lg",
  "label",
  "caption",
  "overline",
  "title-card",
];
const UTILITY_PREFIX =
  "(?:bg|text|border|border-[tblrxyse]|ring|ring-offset|fill|stroke|divide|outline|from|to|via|shadow|decoration|accent|caret|placeholder)";
const RADIUS_SIDE = "(?:-[tblrse]{1,2})?";

/** The patterns that mean an old name, with a label for the failure message. */
export function legacyPatterns(): [string, RegExp][] {
  const any = (names: string[]) => names.join("|");
  return [
    ["custom property", new RegExp(`(?<![\\w-])--(?:${any(VARIABLE_NAMES)})(?![\\w-])`)],
    [
      "colour utility",
      new RegExp(`(?<![\\w-])${UTILITY_PREFIX}-(?:${any(COLOUR_NAMES)})(?![\\w-])`),
    ],
    [
      "radius utility",
      new RegExp(`(?<![\\w-])rounded${RADIUS_SIDE}-(?:xs|sm|md|lg|xl|pill)(?![\\w-])`),
    ],
    ["shadow utility", /(?<![\w-])shadow-(?:poster-hover|poster|thumb)(?![\w-])/],
    ["type class", new RegExp(`(?<![\\w-])type-(?:${any(TYPE_NAMES)})(?![\\w-])`)],
  ];
}

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

describe("the old design-system names", () => {
  it("recognises each kind of old name", () => {
    const patterns = legacyPatterns();
    const hit = (text: string) => patterns.some(([, pattern]) => pattern.test(text));
    expect(hit("background: var(--surface-hi)")).toBe(true);
    expect(hit('className="bg-accent text-on-accent"')).toBe(true);
    expect(hit('className="rounded-md"')).toBe(true);
    expect(hit('className="shadow-poster"')).toBe(true);
    expect(hit('className="type-caption"')).toBe(true);
    expect(hit("transition: var(--duration-base)")).toBe(true);
    expect(hit('className="bg-surface-card text-text-2 rounded-(--r-6) type-small"')).toBe(false);
    expect(hit("border: 1px solid var(--line-section)")).toBe(false);
  });

  it("appear nowhere in ui/src", () => {
    const own = resolve(__dirname, "legacyNames.test.ts");
    const found: string[] = [];
    for (const file of walk(SRC)) {
      if (file === own || !/\.(tsx?|css|json)$/.test(file)) continue;
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, index) => {
        for (const [label, pattern] of legacyPatterns()) {
          if (pattern.test(line))
            found.push(`${relative(SRC, file)}:${index + 1} ${label}: ${line.trim()}`);
        }
      });
    }
    expect(found).toEqual([]);
  });
});
