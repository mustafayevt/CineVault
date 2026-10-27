import { readFileSync } from "node:fs";
import { resolve } from "node:path";

/** Test helpers that read the real `tokens.css` and the design system `tokens.json` copy. */

const stylesDir = resolve(__dirname, "../styles");

export function readStyle(name: string): string {
  return readFileSync(resolve(stylesDir, name), "utf8");
}

export interface DesignTokensJson {
  color: { tokens: { name: string; value: { dark: string; light: string } }[] };
  shadow: { tokens: { name: string; value: { dark: string; light: string } }[] };
  type: {
    families: Record<string, string>;
    groups: {
      family: string;
      styles: {
        name: string;
        fontSize: string;
        lineHeight: number | string;
        fontWeight: number;
        letterSpacing?: string;
      }[];
    }[];
  };
  spacing: { tokens: { name: string; value: string }[] };
  radius: { tokens: { name: string; value: string }[] };
  fontSize: { tokens: { name: string; value: string }[] };
  fontWeight: { tokens: { name: string; value: string }[] };
  size: { tokens: { name: string; value: string }[] };
  duration: { tokens: { name: string; value: string }[] };
  easing: { tokens: { name: string; value: string }[] };
  motion: { tokens: { name: string; value: string }[] };
}

export function readDesignTokens(): DesignTokensJson {
  return JSON.parse(readStyle("design-tokens.json")) as DesignTokensJson;
}

/** Returns the declarations of the first rule whose selector line starts with `selector`. */
export function declarations(css: string, selector: string): Map<string, string> {
  const start = css.indexOf(`${selector} {`);
  if (start < 0) throw new Error(`Selector not found: ${selector}`);
  const end = css.indexOf("\n}", start);
  const body = css.slice(css.indexOf("{", start) + 1, end);
  const result = new Map<string, string>();
  for (const match of body.matchAll(/(--[\w-]+):\s*([^;]+);/g)) {
    const [, name, value] = match;
    if (name && value) result.set(name, value.trim());
  }
  return result;
}

/** Colour tokens for a theme, as `name -> css value`, with the `--` prefix removed. */
export function themeColors(theme: "dark" | "light"): Record<string, string> {
  const css = readStyle("tokens.css");
  const block =
    theme === "dark"
      ? declarations(css, ':root,\n[data-theme="dark"]')
      : declarations(css, '[data-theme="light"]');
  const result: Record<string, string> = {};
  for (const [name, value] of block) result[name.slice(2)] = value;
  return result;
}
