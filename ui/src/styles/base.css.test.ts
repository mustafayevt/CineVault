import { describe, expect, it } from "vitest";

import { readStyle } from "../test/tokenCss";

const css = readStyle("base.css");
const typography = readStyle("typography.css");

/** The declarations of the first rule whose selector text contains `selector`. */
function rule(source: string, selector: string): string {
  const start = source.indexOf(selector);
  expect(start, `${selector} is missing`).toBeGreaterThanOrEqual(0);
  const open = source.indexOf("{", start);
  let depth = 0;
  for (let i = open; i < source.length; i++) {
    if (source[i] === "{") depth++;
    if (source[i] === "}" && --depth === 0) return source.slice(open + 1, i);
  }
  throw new Error(`${selector} is not closed`);
}

/** The selector list that leads to the first rule declaring `declaration`. */
function selectorsOf(source: string, declaration: string): string {
  const at = source.indexOf(declaration);
  expect(at, `${declaration} is missing`).toBeGreaterThanOrEqual(0);
  const open = source.lastIndexOf("{", at);
  const close = source.lastIndexOf("}", open);
  return source.slice(close + 1, open);
}

describe("base stylesheet (desktop-interaction)", () => {
  it("is imported by the app stylesheet", () => {
    expect(readStyle("index.css")).toContain('@import "./base.css"');
  });

  it("makes interface text unselectable and lets values, paths and literals opt back in", () => {
    expect(rule(css, "body {")).toMatch(/user-select:\s*none/);
    const selectable = selectorsOf(css, "user-select: text");
    for (const selector of [".cv-selectable", ".type-code", "input", "textarea", "pre", "code"]) {
      expect(selectable, selector).toContain(selector);
    }
  });

  it("shows the hand on everything clickable and the text cursor only in fields", () => {
    const pointer = selectorsOf(css, "cursor: pointer");
    for (const selector of ["button", '[role="button"]', "a[href]", "[data-clickable]"]) {
      expect(pointer, selector).toContain(selector);
    }
    const text = selectorsOf(css, "cursor: text");
    expect(text).toContain("textarea");
    expect(text).toContain("input:not(");
    expect(text).not.toMatch(/^\s*button/m);
  });

  it("shows not-allowed on disabled controls, after the hand so it wins", () => {
    const disabled = selectorsOf(css, "cursor: not-allowed");
    expect(disabled).toContain(":disabled");
    expect(disabled).toContain('[aria-disabled="true"]');
    expect(css.indexOf("cursor: not-allowed")).toBeGreaterThan(css.indexOf("cursor: pointer"));
  });

  it("shows resize, grab and grabbing cursors", () => {
    expect(rule(css, '[data-resize="col"]')).toMatch(/cursor:\s*col-resize/);
    expect(rule(css, '[data-resize="row"]')).toMatch(/cursor:\s*row-resize/);
    expect(rule(css, "[data-drag-source] {")).toMatch(/cursor:\s*grab/);
    expect(rule(css, '[data-dragging="true"]')).toMatch(/cursor:\s*grabbing/);
  });

  it("never shows the wait cursor", () => {
    expect(css).not.toMatch(/cursor:\s*(wait|progress)/);
  });

  it("does not underline buttons", () => {
    expect(selectorsOf(css, "text-decoration: none")).toContain("button");
  });

  it("stops image ghost drags unless the image is a drag source", () => {
    expect(rule(css, "img {")).toMatch(/-webkit-user-drag:\s*none/);
    expect(rule(css, "img[data-drag-source]")).toMatch(/-webkit-user-drag:\s*element/);
  });

  it("draws thin scrollbars from the line tokens, with a standard-property fallback", () => {
    expect(rule(css, "::-webkit-scrollbar {")).toMatch(/width:\s*10px/);
    expect(rule(css, "::-webkit-scrollbar-thumb {")).toContain("var(--line-kbd)");
    expect(rule(css, "::-webkit-scrollbar-button")).toMatch(/display:\s*none/);
    expect(css).toContain("@supports not selector(::-webkit-scrollbar)");
    expect(css).toContain("scrollbar-width: thin");
  });

  it("reads only tokens: no hex, rgb or hsl values", () => {
    expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}(?![0-9a-zA-Z])|\b(?:rgba?|hsla?)\(/);
  });
});

describe("typography stylesheet (type scale)", () => {
  const code = typography.replace(/\/\*[\s\S]*?\*\//g, "");

  it("uses only the type-scale sizes and the three weights, through tokens", () => {
    const sizes = new Set([...code.matchAll(/var\(--fs-(\d+)\)/g)].map((m) => m[1]));
    expect([...sizes].sort((a, b) => Number(a) - Number(b))).toEqual([
      "11",
      "12",
      "13",
      "14",
      "16",
      "20",
      "28",
    ]);
    const weights = new Set([...code.matchAll(/var\(--fw-(\d+)\)/g)].map((m) => m[1]));
    expect([...weights].sort()).toEqual(["500", "600", "700"]);
    expect(code).not.toMatch(/font:\s*\d/);
  });

  it("has the one section-label style: 11px semibold uppercase, 0.06em, muted", () => {
    const body = rule(typography, ".type-section-label {");
    expect(body).toContain("font: var(--fw-600) var(--fs-11)/");
    expect(body).toMatch(/letter-spacing:\s*0\.06em/);
    expect(body).toMatch(/text-transform:\s*uppercase/);
    expect(body).toMatch(/color:\s*var\(--text-muted\)/);
  });

  it("sets text in Archivo and literal strings in JetBrains Mono, nothing else", () => {
    const families = new Set([...code.matchAll(/var\(--font-([\w-]+)\)/g)].map((m) => m[1]));
    expect([...families].sort()).toEqual(["mono", "sans"]);
    expect(code).not.toMatch(/serif|font-stretch/);
    const mono = [...code.matchAll(/\.type-([\w-]+)\s*\{[^}]*var\(--font-mono\)/g)].map(
      (m) => m[1],
    );
    expect(mono).toEqual(["code", "code-sm"]);
  });
});
