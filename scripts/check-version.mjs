// Checks that the four places that hold the app version agree, and optionally that they say the
// version a release is being cut for.
//
//   node scripts/check-version.mjs            the four files agree with each other
//   node scripts/check-version.mjs 1.2.0      ...and the version is 1.2.0 (a leading v is ignored)
//   node scripts/check-version.mjs --self-test
//
// The files: src-tauri/tauri.conf.json, package.json, ui/package.json and the root Cargo.toml
// ([workspace.package] version). The release workflow runs it on the pushed tag.
import { readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The version of the `[workspace.package]` table in the text of a Cargo.toml, or null. */
export function workspaceVersion(cargoToml) {
  const table = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargoToml);
  const match = table && /^version\s*=\s*"([^"]+)"/m.exec(table[1]);
  return match ? match[1] : null;
}

/** The problems with `found` ({ file: version }) against the optional `wanted` version. */
export function problemsWith(found, wanted) {
  const problems = [];
  for (const [file, version] of Object.entries(found)) {
    if (!version) problems.push(`${file} has no version`);
    else if (wanted && version !== wanted) problems.push(`${file} says ${version}, expected ${wanted}`);
  }
  const distinct = new Set(Object.values(found).filter(Boolean));
  if (!wanted && distinct.size > 1) {
    problems.push(`the version differs between files: ${Object.entries(found).map(([f, v]) => `${f} ${v}`).join(", ")}`);
  }
  return problems;
}

function readVersions() {
  const json = (path) => JSON.parse(readFileSync(join(root, path), "utf8")).version;
  return {
    "src-tauri/tauri.conf.json": json("src-tauri/tauri.conf.json"),
    "package.json": json("package.json"),
    "ui/package.json": json("ui/package.json"),
    "Cargo.toml": workspaceVersion(readFileSync(join(root, "Cargo.toml"), "utf8")),
  };
}

function selfTest() {
  const assert = (ok, message) => {
    if (!ok) throw new Error(`self-test failed: ${message}`);
  };
  assert(workspaceVersion('[workspace]\nmembers = []\n\n[workspace.package]\nedition = "2024"\nversion = "1.2.3"\n\n[workspace.dependencies]\nversion = "9"\n') === "1.2.3", "reads the workspace.package version");
  assert(workspaceVersion('[workspace.package]\nversion = "0.1.0"') === "0.1.0", "table at the end of the file");
  assert(workspaceVersion('[package]\nversion = "1.0.0"\n') === null, "no workspace.package table");
  const same = { a: "1.2.0", b: "1.2.0" };
  assert(problemsWith(same).length === 0 && problemsWith(same, "1.2.0").length === 0, "agreeing files pass");
  assert(problemsWith({ a: "1.2.0", b: "1.1.0" }).length === 1, "files that differ are reported");
  assert(problemsWith(same, "1.3.0").length === 2, "every file is checked against the wanted version");
  assert(problemsWith({ a: null }).length === 1, "a missing version is reported");
  const real = problemsWith(readVersions());
  assert(real.length === 0, `the repository's own files agree (${real.join("; ")})`);
  console.log("check-version self-test passed");
}

const args = process.argv.slice(2);
if (args[0] === "--self-test") {
  selfTest();
} else {
  const wanted = args[0]?.replace(/^v/, "");
  const found = readVersions();
  const problems = problemsWith(found, wanted);
  if (problems.length) {
    for (const problem of problems) console.error(`version check: ${problem}`);
    process.exit(1);
  }
  console.log(`version check passed: ${Object.values(found)[0]}${wanted ? " (as requested)" : ""}`);
}
