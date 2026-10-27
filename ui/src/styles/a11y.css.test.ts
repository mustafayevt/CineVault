import { describe, expect, it } from "vitest";

import { readStyle } from "../test/tokenCss";

const css = readStyle("a11y.css");

/** The text of the first at-rule block that starts with `header`, braces balanced. */
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

describe("accessibility stylesheet", () => {
  it("keeps every menu inside the window: as tall as the room Radix measured, scrolling inside", () => {
    const menu = block(css, "[data-radix-menu-content]");
    expect(menu).toMatch(/--radix-dropdown-menu-content-available-height/);
    expect(menu).toMatch(/--radix-context-menu-content-available-height/);
    expect(menu).toMatch(/overflow-y:\s*auto/);
  });

  it("is imported by the app stylesheet", () => {
    expect(readStyle("index.css")).toContain('@import "./a11y.css"');
  });

  it("draws the focus ring from the focus-ring token, 2px wide and 2px off", () => {
    const base = block(css, "@layer base");
    expect(block(base, ":focus-visible")).toMatch(/outline:\s*2px solid var\(--focus-ring\)/);
    expect(block(base, ":focus-visible")).toMatch(/outline-offset:\s*2px/);
  });

  it("starts every element from the ring colour, so a colour transition cannot tint the ring", () => {
    const base = block(css, "@layer base");
    const all = block(base, "*,");
    expect(all).toMatch(/outline-color:\s*var\(--focus-ring\)/);
  });

  it("draws the ring for keyboard focus only: a click leaves none", () => {
    const base = block(css, "@layer base");
    expect(block(base, ":focus:not(:focus-visible)")).toMatch(/outline:\s*none/);
    expect(css).not.toMatch(/(?<!:not\()(?<![\w-]):focus\s*\{[^}]*outline:\s*2px/);
  });

  it("marks a focused field with the gold border, clicked or tabbed into", () => {
    const rule = block(css, "[data-field]:focus-within");
    expect(rule).toMatch(/border-color:\s*var\(--gold\)/);
    expect(css).toMatch(/:is\(input, textarea, select\):not\(/);
    // Outside any layer, so a utility's border colour cannot beat it.
    expect(block(css, "@layer base")).not.toContain("[data-field]:focus-within");
    expect(block(css, "[data-field] :is(input, textarea, select):focus-visible {")).toMatch(
      /outline:\s*none/,
    );
  });

  it("restyles for forced colours with system colours only", () => {
    const forced = block(css, "@media (forced-colors: active)");
    expect(forced).toContain("outline: 2px solid Highlight");
    expect(forced).toContain('[aria-pressed="true"]');
    expect(forced).toContain("HighlightText");
    expect(forced).toContain("GrayText");
  });

  it("has no hard-coded hex colours", () => {
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
  });
});
