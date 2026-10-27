import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative, resolve, sep } from "node:path";

import { describe, expect, it } from "vitest";

import { allMessages } from "./catalog";

const SRC = resolve(__dirname, "..");

function walk(dir: string, out: string[] = []): string[] {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) walk(path, out);
    else out.push(path);
  }
  return out;
}

/** The source the UI ships: no tests, no catalogue files, no generated bindings. */
function shippedSource(): string {
  return walk(SRC)
    .filter(
      (file) =>
        /\.tsx?$/.test(file) &&
        !/\.test\.tsx?$/.test(file) &&
        !relative(SRC, file).startsWith(`i18n${sep}messages`) &&
        !file.endsWith("bindings.ts"),
    )
    .map((file) => readFileSync(file, "utf8"))
    .join("\n");
}

const TEMPLATE = /`([a-z][A-Za-z0-9]*\.[^`\n]*\$\{[^`\n]*)`/g;

/**
 * The string catalogue holds no dead text: every key is written out in the shipped
 * source as a quoted string, built from a template (`` `shell.nav.${id}` ``) or reached through a
 * quoted prefix that ends in a dot (`"shell.nav." + id`). A message nothing reads is deleted with
 * the page that showed it.
 */
describe("the string catalogue", () => {
  it("has no message that nothing uses", () => {
    const text = shippedSource();
    const templates: RegExp[] = [];
    for (const match of text.matchAll(TEMPLATE)) {
      const pattern = (match[1] ?? "")
        .split(/\$\{[^}]*\}/)
        .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
        .join("[A-Za-z0-9_.-]+");
      templates.push(new RegExp(`^${pattern}$`));
    }
    const quoted = (value: string): boolean =>
      text.includes(`"${value}"`) || text.includes(`'${value}'`) || text.includes(`\`${value}\``);

    const unused = Object.keys(allMessages()).filter((key) => {
      if (quoted(key) || templates.some((pattern) => pattern.test(key))) return false;
      const parts = key.split(".");
      for (let i = 1; i < parts.length; i++) {
        if (quoted(`${parts.slice(0, i).join(".")}.`)) return false;
      }
      return true;
    });
    expect(unused).toEqual([]);
  });
});
