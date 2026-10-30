// Checks that every `bun run <name>` named in AGENTS.md or README.md exists in the root or the
// ui/ package.json. A renamed script otherwise leaves a command in the docs that nobody can run.
//
//   bun run check:scripts
//   node scripts/check-scripts.mjs --self-test
import { existsSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const docs = ["AGENTS.md", "README.md"];
const packageFiles = ["package.json", join("ui", "package.json")];

/** The script names a `bun run` command mentions in prose or a code block. */
export function commandNames(text) {
  const names = new Set();
  for (const match of text.matchAll(/\bbun\s+run\s+([A-Za-z0-9][A-Za-z0-9:_-]*)/g)) {
    names.add(match[1]);
  }
  return names;
}

/** The names that are in neither package's `scripts` table. */
export function missingCommands(names, scripts) {
  return [...names].filter((name) => !scripts.has(name)).sort();
}

function readScripts() {
  const scripts = new Set();
  for (const file of packageFiles) {
    const path = join(root, file);
    if (!existsSync(path)) throw new Error(`missing ${file}`);
    const packageJson = JSON.parse(readFileSync(path, "utf8"));
    for (const name of Object.keys(packageJson.scripts ?? {})) scripts.add(name);
  }
  return scripts;
}

function selfTest() {
  const assert = (ok, message) => {
    if (!ok) throw new Error(`self-test failed: ${message}`);
  };
  const names = commandNames(
    "Run `bun run site:changelog -- --check`, then bun run tauri dev; bun run format / bun run format:check.",
  );
  assert(names.has("site:changelog"), "finds a colon name");
  assert(names.has("tauri"), "stops at the argument");
  assert(names.has("format") && names.has("format:check"), "finds both names around a slash");
  assert(!commandNames("bun install").size, "ignores bun install");
  const scripts = new Set(["site:changelog", "tauri"]);
  assert(missingCommands(names, scripts).join(",") === "format,format:check", "reports only the missing names");
  console.log("check-scripts self-test passed");
}

const args = process.argv.slice(2);
if (args[0] === "--self-test") {
  selfTest();
} else {
  const scripts = readScripts();
  const names = new Set();
  for (const file of docs) {
    const path = join(root, file);
    if (!existsSync(path)) {
      console.error(`check-scripts: missing ${file}`);
      process.exit(1);
    }
    for (const name of commandNames(readFileSync(path, "utf8"))) names.add(name);
  }
  const missing = missingCommands(names, scripts);
  if (missing.length) {
    for (const name of missing) console.error(`check-scripts: "${name}" is named in the docs but missing from package.json or ui/package.json`);
    process.exit(1);
  }
  console.log(`check-scripts passed: ${names.size} documented bun command${names.size === 1 ? "" : "s"} exist`);
}
