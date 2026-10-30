// Checks that the places that hold the app version agree, and optionally that they say the version
// a release is being cut for.
//
//   node scripts/check-version.mjs            the files agree with each other
//   node scripts/check-version.mjs 1.2.0      ...and the version is 1.2.0 (a leading v is ignored)
//   node scripts/check-version.mjs --server-artifacts <dir> [--server-artifacts <dir> ...]
//                                             ...and the server packages/tarball in <dir> name it
//   node scripts/check-version.mjs --self-test
//
// The files: src-tauri/tauri.conf.json, package.json, ui/package.json, the root Cargo.toml
// ([workspace.package] version) and the Dockerfile's CINEVAULT_VERSION build argument (the image
// tag). The server package and tarball names are generated from the same Cargo version by
// scripts/package-server.sh and scripts/server-tarball.sh. The release workflow runs this on the
// pushed tag and passes the version to the image build.
import { existsSync, readFileSync, readdirSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** The version of the `[workspace.package]` table in the text of a Cargo.toml, or null. */
export function workspaceVersion(cargoToml) {
  const table = /^\[workspace\.package\]\s*$([\s\S]*?)(?=^\[|(?![\s\S]))/m.exec(cargoToml);
  const match = table && /^version\s*=\s*"([^"]+)"/m.exec(table[1]);
  return match ? match[1] : null;
}

/** The default of the Dockerfile's `CINEVAULT_VERSION` build argument, or null. */
export function dockerfileVersion(dockerfile) {
  const match = /^ARG\s+CINEVAULT_VERSION=([^\s#]+)/m.exec(dockerfile);
  return match ? match[1].replace(/^["']|["']$/g, "") : null;
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

/** Whether a server package or tarball name carries `version` as its own part. */
function namesVersion(name, version) {
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|[_-])${escaped}(?=[_.-])`).test(name);
}

/**
 * The problems with the server artifact names in `files`: the .deb, .rpm and tarball must carry
 * the version, so a stale `target/server-package` cannot be attached to a release. Extra files are
 * ignored; at least one server artifact has to be present.
 */
export function serverArtifactProblems(files, version) {
  const artifacts = files.filter(
    (name) =>
      /^cinevault-server_.+\.deb$/i.test(name) ||
      /^cinevault-server-.+\.rpm$/i.test(name) ||
      /^cinevault-server-.+\.tar\.gz$/i.test(name),
  );
  const problems = [];
  if (artifacts.length === 0) problems.push("no cinevault-server .deb, .rpm or tarball was found");
  for (const name of artifacts) {
    if (!namesVersion(name, version)) problems.push(`${name} does not name version ${version}`);
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
    Dockerfile: dockerfileVersion(readFileSync(join(root, "Dockerfile"), "utf8")),
  };
}

function selfTest() {
  const assert = (ok, message) => {
    if (!ok) throw new Error(`self-test failed: ${message}`);
  };
  assert(workspaceVersion('[workspace]\nmembers = []\n\n[workspace.package]\nedition = "2024"\nversion = "1.2.3"\n\n[workspace.dependencies]\nversion = "9"\n') === "1.2.3", "reads the workspace.package version");
  assert(workspaceVersion('[workspace.package]\nversion = "0.1.0"') === "0.1.0", "table at the end of the file");
  assert(workspaceVersion('[package]\nversion = "1.0.0"\n') === null, "no workspace.package table");
  assert(dockerfileVersion('ARG CINEVAULT_VERSION=1.2.3\nFROM debian:bookworm-slim\n') === "1.2.3", "reads the image version argument");
  assert(dockerfileVersion('FROM debian:bookworm-slim\n') === null, "no image version argument");
  const artifacts = [
    "cinevault-server_1.2.3_amd64.deb",
    "cinevault-server-1.2.3-1.x86_64.rpm",
    "cinevault-server-1.2.3-linux-amd64.tar.gz",
    "notes.txt",
  ];
  assert(serverArtifactProblems(artifacts, "1.2.3").length === 0, "server artifacts carry the version");
  assert(serverArtifactProblems(artifacts, "1.2.4").length === 3, "every server artifact is checked");
  assert(serverArtifactProblems([], "1.2.3").length === 1, "missing server artifacts are reported");
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
if (args.includes("--self-test")) {
  selfTest();
} else {
  let wanted = null;
  const artifactDirs = [];
  for (let index = 0; index < args.length; index += 1) {
    const arg = args[index];
    if (arg === "--server-artifacts") {
      const dir = args[index + 1];
      if (!dir) {
        console.error("version check: --server-artifacts needs a folder");
        process.exit(2);
      }
      artifactDirs.push(dir);
      index += 1;
    } else if (arg.startsWith("-")) {
      console.error(`version check: unknown option ${arg}`);
      process.exit(2);
    } else if (wanted === null) {
      wanted = arg.replace(/^v/, "");
    } else {
      console.error(`version check: unexpected argument ${arg}`);
      process.exit(2);
    }
  }

  const found = readVersions();
  const problems = problemsWith(found, wanted);
  if (artifactDirs.length > 0) {
    const files = [];
    for (const dir of artifactDirs) {
      if (!existsSync(dir)) {
        problems.push(`${dir} does not exist`);
      } else {
        files.push(...readdirSync(dir));
      }
    }
    problems.push(...serverArtifactProblems(files, wanted ?? found["Cargo.toml"]));
  }
  if (problems.length) {
    for (const problem of problems) console.error(`version check: ${problem}`);
    process.exit(1);
  }
  console.log(`version check passed: ${Object.values(found)[0]}${wanted ? " (as requested)" : ""}`);
}
