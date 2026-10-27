// Publishes a built release to the public update channel. Maintainers only: it runs on the
// machine that holds the update-signing private key, with the `gh` CLI signed in; the key never
// leaves that machine. The key path defaults to ~/.cinevault-keys/cinevault-update.pem and can be
// overridden with --key or the CINEVAULT_UPDATE_KEY environment variable.
//
//   node scripts/release-publish.mjs --from-dir <folder> [--from-dir <folder> ...] [--version <x.y.z>]
//       [--notes <file>] [--key <private-key.pem>] [--dry-run]
//   node scripts/release-publish.mjs [--version <x.y.z>] [--notes <file>] [--key <private-key.pem>] [--dry-run]
//   node scripts/release-publish.mjs --self-test
//
// Where the files come from:
// - --from-dir (repeatable): the installers built on this machine, or in WSL for Linux, read from
//   those folders as they are (not recursively: give target/release/bundle/nsis, .../msi, and so on).
//   Only the files of --version are taken, so older builds left in a folder are skipped. The
//   checksum files SHA256SUMS-Windows.txt, -macOS.txt and -Linux.txt are made here from those files.
// - no --from-dir: the assets of Release v<version> in the source repository, as the release
//   workflows (release-windows.yml, release-macos.yml) made them. Their SHA256SUMS-<os>.txt files
//   are checked against the downloaded installers.
//
// Then, for either source:
// 1. Checks every installer against the SHA256SUMS-<os>.txt files.
// 2. Writes the manifest payload (version, date, notes, one build per platform: the NSIS setup
//    exe for Windows, the DMG for macOS and the AppImage for Linux when they are there), signs it
//    with the private key and verifies the envelope with the public key built into the app.
//    In local mode the manifest is written from the folders given, so pass every folder of the
//    version each time: a platform left out of them is left out of the manifest.
// 3. Creates or updates Release v<version> in the public releases repository with the installers,
//    the sums and manifest.json, and marks it latest. Rerunning is safe (--clobber); run it again
//    after the macOS build to add the darwin builds.
// 4. Uploads version-less copies of the installers (CineVault-setup-x64.exe, CineVault-x64.msi,
//    CineVault-portable-x64.zip, CineVault-universal.dmg, CineVault-x86_64.AppImage,
//    CineVault-amd64.deb, CineVault-x86_64.rpm), so that .../releases/latest/download/<name> is a
//    stable download link for the website.
//
// --dry-run does steps 1 and 2 and uploads nothing.
import { execFileSync } from "node:child_process";
import { createHash, generateKeyPairSync } from "node:crypto";
import { copyFileSync, createReadStream, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { homedir, tmpdir } from "node:os";
import { basename, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  builtInPublicKey,
  insideRepo,
  loadPrivateKey,
  publicKeyText,
  rawPublicKey,
  repoRoot,
  signPayload,
  verifyEnvelope,
} from "./update-signing.mjs";

export const SOURCE_REPO = "mustafayevt/CineVault";
export const RELEASES_REPO = "mustafayevt/CineVault-releases";

// ---------------------------------------------------------------------------------------------
// Pure parts (covered by --self-test)

/** Reads a `sha256sum` / `shasum -a 256` listing into Map<file name, lower-case hex>. */
export function parseSums(text) {
  const sums = new Map();
  for (const line of text.split(/\r?\n/)) {
    if (line.trim() === "") continue;
    const match = /^([0-9a-fA-F]{64})\s+\*?(.+?)\s*$/.exec(line);
    if (!match) throw new Error(`unreadable checksum line: ${line}`);
    sums.set(match[2], match[1].toLowerCase());
  }
  return sums;
}

/** The architecture word of a Linux file name: `aarch64` for arm64, `x86_64` otherwise. */
const linuxArch = (name) => (/arm64|aarch64/i.test(name) ? "aarch64" : "x86_64");

/**
 * What a release file is: `sums`, an installer that goes into the manifest (`platforms`), an
 * installer that is only uploaded, or `null` for anything else. `os` is the operating system the
 * file is for; it decides which SHA256SUMS file lists the file.
 */
export function classifyAsset(name) {
  if (/^SHA256SUMS-.+\.txt$/i.test(name)) return { kind: "sums" };
  if (/-setup\.exe$/i.test(name)) {
    // The NSIS installer: per user, upgrades an older install in place (and offers to remove an
    // MSI install first), so it is the one the app hands to Windows.
    const arch = /arm64|aarch64/i.test(name) ? "aarch64" : "x86_64";
    return { kind: "installer", os: "Windows", platforms: [`windows-${arch}`] };
  }
  if (/\.dmg$/i.test(name)) {
    const os = "macOS";
    if (/universal/i.test(name)) return { kind: "installer", os, platforms: ["darwin-aarch64", "darwin-x86_64"] };
    if (/aarch64|arm64/i.test(name)) return { kind: "installer", os, platforms: ["darwin-aarch64"] };
    return { kind: "installer", os, platforms: ["darwin-x86_64"] };
  }
  // The AppImage is the installer the app updates itself with. Tauri puts an underscore after
  // the product name in the versioned file; the version-less copy (CineVault-x86_64.AppImage) does
  // not, so it is not a second installer.
  if (/^CineVault_.+\.AppImage$/i.test(name)) {
    return { kind: "installer", os: "Linux", platforms: [`linux-${linuxArch(name)}`] };
  }
  // The .deb and .rpm packages are for package managers: uploaded, not in the manifest.
  if (/\.(deb|rpm)$/i.test(name)) return { kind: "upload", os: "Linux" };
  if (/\.msi$/i.test(name) || /-portable\.zip$/i.test(name)) return { kind: "upload", os: "Windows" };
  return null;
}

/**
 * The version-less name of an installer, uploaded next to it so that
 * `releases/latest/download/<name>` always serves the newest build; `null` for other files.
 */
export function stableName(name) {
  if (/^CineVault_.+\.AppImage$/i.test(name)) return `CineVault-${linuxArch(name)}.AppImage`;
  if (/\.deb$/i.test(name)) return `CineVault-${/arm64|aarch64/i.test(name) ? "arm64" : "amd64"}.deb`;
  if (/\.rpm$/i.test(name)) return `CineVault-${linuxArch(name)}.rpm`;
  const arch = /arm64|aarch64/i.test(name) ? "arm64" : "x64";
  if (/-setup\.exe$/i.test(name)) return `CineVault-setup-${arch}.exe`;
  if (/\.msi$/i.test(name)) return `CineVault-${arch}.msi`;
  if (/-portable\.zip$/i.test(name)) return `CineVault-portable-${arch}.zip`;
  if (/\.dmg$/i.test(name)) return /universal/i.test(name) ? "CineVault-universal.dmg" : `CineVault-${arch}.dmg`;
  return null;
}

/**
 * Whether a file name carries `version` as a part of its own, in either naming style:
 * CineVault_1.2.0_x64-setup.exe or CineVault-1.2.0-1.x86_64.rpm. 1.2.10 does not match 1.2.1.
 */
export function namesVersion(name, version) {
  const escaped = version.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  return new RegExp(`(^|[_-])${escaped}(?=[_.-])`).test(name);
}

/** The download address of a file in the public release. */
export const assetUrl = (version, name, repo = RELEASES_REPO) =>
  `https://github.com/${repo}/releases/download/v${version}/${encodeURIComponent(name)}`;

/**
 * Checks the files against the sums. `files` is [{ name, sha256, size }] for everything
 * downloaded; `sums` maps a name to its expected hash. Every installer must be listed and match,
 * and every listed file must be there. Returns the problems (empty when all is well).
 */
export function checkAgainstSums(files, sums) {
  const problems = [];
  const byName = new Map(files.map((file) => [file.name, file]));
  for (const [name, expected] of sums) {
    const file = byName.get(name);
    if (!file) problems.push(`${name} is listed in the checksums but was not downloaded`);
    else if (file.sha256 !== expected) problems.push(`${name} does not match its checksum`);
  }
  for (const file of files) {
    const kind = classifyAsset(file.name)?.kind;
    if ((kind === "installer" || kind === "upload") && !sums.has(file.name)) {
      problems.push(`${file.name} is not listed in any SHA256SUMS file`);
    }
  }
  return problems;
}

/** The manifest payload: version, date, notes and one signed build per platform. */
export function buildPayload({ version, pubDate, notes, files, repo = RELEASES_REPO }) {
  const platforms = {};
  for (const file of [...files].sort((a, b) => a.name.localeCompare(b.name))) {
    const kind = classifyAsset(file.name);
    if (kind?.kind !== "installer") continue;
    for (const platform of kind.platforms) {
      if (platforms[platform]) throw new Error(`two installers for ${platform}: ${platforms[platform].file} and ${file.name}`);
      platforms[platform] = { file: file.name, url: assetUrl(version, file.name, repo), sha256: file.sha256, size: file.size };
    }
  }
  if (Object.keys(platforms).length === 0) throw new Error(`release v${version} has no installer for the update channel (setup exe or DMG)`);
  const sorted = Object.fromEntries(
    Object.keys(platforms)
      .sort()
      .map((id) => {
        const { url, sha256, size } = platforms[id];
        return [id, { url, sha256, size }];
      }),
  );
  return { version, pub_date: pubDate, notes, platforms: sorted };
}

/** A release body worth showing: the generated "Full Changelog" link alone is not. */
export function meaningfulBody(body) {
  const kept = String(body ?? "")
    .split(/\r?\n/)
    .filter((line) => !/^\s*\*\*Full Changelog\*\*/i.test(line))
    .join("\n")
    .trim();
  return kept === "" ? null : kept;
}

/** Release notes from commit subjects: feat under "New", fix under "Fixes", the rest left out. */
export function notesFromCommits(subjects) {
  const groups = { feat: [], fix: [] };
  for (const subject of subjects) {
    const match = /^(feat|fix)(?:\([^)]*\))?!?:\s*(.+)$/.exec(subject.trim());
    if (!match) continue;
    const text = match[2].trim();
    const line = `- ${text.charAt(0).toUpperCase()}${text.slice(1)}`;
    if (!groups[match[1]].includes(line)) groups[match[1]].push(line);
  }
  const parts = [];
  if (groups.feat.length) parts.push(["New", ...groups.feat].join("\n"));
  if (groups.fix.length) parts.push(["Fixes", ...groups.fix].join("\n"));
  return parts.join("\n\n");
}

const semverParts = (tag) => {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(tag);
  return match ? match.slice(1, 4).map(Number) : null;
};
const compareParts = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

/** The newest plain release tag older than `version`, or null. */
export function previousTag(tags, version) {
  const current = semverParts(version);
  let best = null;
  for (const tag of tags) {
    const parts = semverParts(tag);
    if (!parts || compareParts(parts, current) >= 0) continue;
    if (!best || compareParts(parts, best.parts) > 0) best = { tag, parts };
  }
  return best?.tag ?? null;
}

// ---------------------------------------------------------------------------------------------
// Machine parts

function parseArgs(argv) {
  const options = { dryRun: false, selfTest: false, fromDirs: [] };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      return next;
    };
    if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--self-test") options.selfTest = true;
    else if (arg === "--version") options.version = value().replace(/^v/, "");
    else if (arg === "--notes") options.notes = value();
    else if (arg === "--key") options.key = value();
    else if (arg === "--from-dir") options.fromDirs.push(value());
    else throw new Error(`unknown option ${arg}`);
  }
  return options;
}

const gh = (args, opts = {}) =>
  execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024, ...opts });

const sha256File = (path) =>
  new Promise((done, fail) => {
    const hash = createHash("sha256");
    createReadStream(path)
      .on("data", (chunk) => hash.update(chunk))
      .on("error", fail)
      .on("end", () => done(hash.digest("hex")));
  });

/** Name, path, size and SHA-256 of a file. */
async function describeFile(path) {
  return { name: basename(path), path, size: statSync(path).size, sha256: await sha256File(path) };
}

/**
 * The installers and packages of `version` in the build output folders, as [{ name, path }].
 * A folder is read as it is, not recursively. Other versions left in a folder are reported in
 * `skipped`; a file name found in two folders is refused, since either copy could be the build.
 */
export function findLocalFiles(folders, version) {
  const found = new Map();
  const skipped = [];
  for (const folder of folders) {
    if (!existsSync(folder) || !statSync(folder).isDirectory()) throw new Error(`${folder} is not a folder`);
    for (const name of readdirSync(folder).sort()) {
      const kind = classifyAsset(name)?.kind;
      if (kind !== "installer" && kind !== "upload") continue;
      const path = join(folder, name);
      if (!statSync(path).isFile()) continue;
      if (!namesVersion(name, version)) {
        skipped.push(name);
        continue;
      }
      if (found.has(name)) throw new Error(`${name} is in two folders: ${found.get(name).path} and ${path}`);
      found.set(name, { name, path });
    }
  }
  return { files: [...found.values()], skipped };
}

/**
 * Local mode: copies the version's files from the build folders into `dir`, writes the checksum
 * files for them (SHA256SUMS-<os>.txt, the names the release workflows use) and returns every
 * file described, checksum files included. The sums are of the copies, so they match what is uploaded.
 */
async function fromBuildFolders(folders, version, dir) {
  const { files: found, skipped } = findLocalFiles(folders, version);
  for (const name of skipped) console.log(`  skipped ${name} (not version ${version})`);
  if (found.length === 0) throw new Error(`no CineVault ${version} installers or packages in ${folders.join(", ")}`);
  const files = [];
  for (const { name, path } of found) {
    const copy = join(dir, name);
    copyFileSync(path, copy);
    files.push(await describeFile(copy));
  }
  for (const os of ["Windows", "macOS", "Linux"]) {
    const listed = files.filter((file) => classifyAsset(file.name)?.os === os);
    if (listed.length === 0) continue;
    const sumsPath = join(dir, `SHA256SUMS-${os}.txt`);
    writeFileSync(sumsPath, listed.map((file) => `${file.sha256}  ${file.name}\n`).join(""));
    files.push(await describeFile(sumsPath));
  }
  return files;
}

export function defaultKeyPath() {
  if (process.env.CINEVAULT_UPDATE_KEY) return process.env.CINEVAULT_UPDATE_KEY;
  return join(homedir(), ".cinevault-keys", "cinevault-update.pem");
}

function releaseNotes(options, version, body) {
  if (options.notes) return { notes: readFileSync(options.notes, "utf8").trim(), from: options.notes };
  const fromBody = meaningfulBody(body);
  if (fromBody) return { notes: fromBody, from: "the release's description" };
  try {
    const tags = gh(["api", `repos/${SOURCE_REPO}/tags`, "--paginate", "--jq", ".[].name"]).split(/\r?\n/).filter(Boolean);
    const previous = previousTag(tags, version);
    // A local build can come before its tag exists; the notes then run up to main.
    const end = tags.includes(`v${version}`) ? `v${version}` : "main";
    const range = previous ? `${previous}...${end}` : null;
    if (range) {
      const subjects = gh([
        "api",
        `repos/${SOURCE_REPO}/compare/${range}?per_page=100`,
        "--paginate",
        "--jq",
        '.commits[].commit.message | split("\\n")[0]',
      ])
        .split(/\r?\n/)
        .filter(Boolean);
      return { notes: notesFromCommits(subjects), from: `the commits ${range}` };
    }
  } catch (error) {
    console.warn(`Could not read the commits for the release notes: ${error.message.split("\n")[0]}`);
  }
  return { notes: "", from: "nothing (no notes found)" };
}

async function publish(options) {
  const version = options.version ?? JSON.parse(readFileSync(resolve(repoRoot, "src-tauri/tauri.conf.json"), "utf8")).version;
  const tag = `v${version}`;
  const keyPath = options.key ?? defaultKeyPath();
  if (insideRepo(keyPath)) throw new Error("the private key must not be inside the repository");
  if (!existsSync(keyPath)) throw new Error(`no private key at ${keyPath} (use --key or CINEVAULT_UPDATE_KEY)`);
  const publicKey = builtInPublicKey();
  if (!publicKey) throw new Error("BUILT_IN_PUBLIC_KEY is not set in crates/cv-app/src/updates/manifest.rs");

  console.log(`CineVault ${version}${options.dryRun ? " (dry run: nothing is uploaded)" : ""}`);
  const local = options.fromDirs.length > 0;
  const release = local ? null : JSON.parse(gh(["release", "view", tag, "-R", SOURCE_REPO, "--json", "body,publishedAt,assets"]));
  const work = mkdtempSync(join(tmpdir(), `cinevault-publish-${version}-`));
  const assetsDir = join(work, "assets");
  mkdirSync(assetsDir);
  let files;
  if (local) {
    console.log(`Reading the ${version} installers from ${options.fromDirs.join(", ")}`);
    files = await fromBuildFolders(options.fromDirs.map((folder) => resolve(folder)), version, assetsDir);
  } else {
    console.log(`Downloading ${release.assets.length} files of ${SOURCE_REPO} ${tag} to ${assetsDir}`);
    gh(["release", "download", tag, "-R", SOURCE_REPO, "-D", assetsDir, "--clobber"], { stdio: ["ignore", "pipe", "inherit"] });
    files = [];
    for (const name of readdirSync(assetsDir).sort()) files.push(await describeFile(join(assetsDir, name)));
  }
  const sums = new Map();
  for (const file of files.filter((f) => classifyAsset(f.name)?.kind === "sums")) {
    for (const [name, hash] of parseSums(readFileSync(file.path, "utf8"))) sums.set(name, hash);
  }
  if (sums.size === 0) throw new Error(`${tag} has no SHA256SUMS file`);
  const problems = checkAgainstSums(files, sums);
  if (problems.length) throw new Error(`checksum check failed:\n  ${problems.join("\n  ")}`);
  for (const file of files) {
    const kind = classifyAsset(file.name);
    if (!kind) console.warn(`  skipped ${file.name} (not an installer or checksum file)`);
    else if (kind.kind !== "sums") console.log(`  ok  ${file.name}  ${file.size} bytes  sha256 ${file.sha256}`);
  }

  const { notes, from } = releaseNotes(options, version, release?.body);
  const payload = buildPayload({ version, pubDate: release?.publishedAt ?? new Date().toISOString(), notes, files });
  if (local) {
    const missing = ["Windows", "macOS", "Linux"].filter((os) => !files.some((f) => classifyAsset(f.name)?.os === os));
    if (missing.length) {
      console.warn(
        `\nWarning: no ${missing.join(" or ")} files in these folders. The manifest is written from these folders alone, ` +
          `so the platforms of ${missing.join(" and ")} are left out of it. Add their folders if the release has them.`,
      );
    }
  }
  const payloadBytes = Buffer.from(JSON.stringify(payload, null, 2) + "\n", "utf8");
  writeFileSync(join(work, "payload.json"), payloadBytes);

  const privateKey = loadPrivateKey(keyPath);
  if (publicKeyText(privateKey) !== publicKey) {
    throw new Error("the private key does not belong to BUILT_IN_PUBLIC_KEY; installed apps would refuse this manifest");
  }
  const envelope = signPayload(payloadBytes, privateKey);
  const checked = verifyEnvelope(envelope, publicKey);
  if (JSON.stringify(checked) !== JSON.stringify(payload)) throw new Error("the signed payload does not read back the same");
  const manifestPath = join(work, "manifest.json");
  writeFileSync(manifestPath, envelope + "\n");

  console.log(`\nNotes from ${from}:\n${notes === "" ? "(none)" : notes.replace(/^/gm, "  ")}`);
  console.log(`\nManifest (signature verified with the built-in public key):`);
  for (const [platform, build] of Object.entries(payload.platforms)) {
    console.log(`  ${platform}: ${build.url} (${build.size} bytes)`);
  }
  console.log(`  written to ${manifestPath}`);

  const uploads = files.filter((f) => classifyAsset(f.name)).map((f) => f.path);
  // Version-less copies for the stable website links (releases/latest/download/<name>).
  const stableDir = join(work, "stable");
  mkdirSync(stableDir);
  const stableUploads = [];
  for (const file of files) {
    const stable = stableName(file.name);
    if (!stable || classifyAsset(file.name)?.kind === "sums") continue;
    const target = join(stableDir, stable);
    if (stableUploads.includes(target)) throw new Error(`two installers would be uploaded as ${stable}`);
    copyFileSync(file.path, target);
    stableUploads.push(target);
    console.log(`  stable copy  ${stable}  (from ${file.name})`);
  }
  if (options.dryRun) {
    const count = uploads.length + stableUploads.length + 1;
    console.log(`\nDry run: would upload ${count} files to ${RELEASES_REPO} ${tag} and mark it latest.`);
    return;
  }

  const notesPath = join(work, "notes.md");
  writeFileSync(notesPath, (notes === "" ? `CineVault ${version}` : notes) + "\n");
  let exists = true;
  try {
    gh(["release", "view", tag, "-R", RELEASES_REPO, "--json", "tagName"]);
  } catch {
    exists = false;
  }
  if (!exists) {
    // A draft first, so the manifest is never public before the installers it names.
    gh(["release", "create", tag, "-R", RELEASES_REPO, "--title", `CineVault ${version}`, "--notes-file", notesPath, "--draft"]);
  }
  console.log(`\nUploading ${uploads.length} installers and checksum files to ${RELEASES_REPO} ${tag}`);
  gh(["release", "upload", tag, "-R", RELEASES_REPO, "--clobber", ...uploads], { stdio: ["ignore", "inherit", "inherit"] });
  if (stableUploads.length) {
    console.log(`Uploading ${stableUploads.length} version-less copies for the stable download links`);
    gh(["release", "upload", tag, "-R", RELEASES_REPO, "--clobber", ...stableUploads], { stdio: ["ignore", "inherit", "inherit"] });
  }
  gh(["release", "upload", tag, "-R", RELEASES_REPO, "--clobber", manifestPath], { stdio: ["ignore", "inherit", "inherit"] });
  gh(["release", "edit", tag, "-R", RELEASES_REPO, "--title", `CineVault ${version}`, "--notes-file", notesPath, "--draft=false", "--latest"]);

  // Read the channel back the way the app does (anonymously, following the redirects).
  const channel = `https://github.com/${RELEASES_REPO}/releases/latest/download/manifest.json`;
  const response = await fetch(channel, { redirect: "follow" });
  const served = response.ok ? (await response.text()).trim() : "";
  if (served !== envelope) {
    console.warn(`\nWarning: ${channel} does not serve the new manifest yet (HTTP ${response.status}). Check the release page.`);
  } else {
    console.log(`\nPublished. ${channel} serves the signed manifest for ${version}.`);
  }
  rmSync(work, { recursive: true, force: true });
}

// ---------------------------------------------------------------------------------------------
// Self-test: node scripts/release-publish.mjs --self-test

async function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const hex = (text) => createHash("sha256").update(text).digest("hex");

  const sums = parseSums(`${hex("exe")} *CineVault_1.2.0_x64-setup.exe\n${hex("msi")}  CineVault_1.2.0_x64_en-US.msi\r\n`);
  assert(sums.get("CineVault_1.2.0_x64-setup.exe") === hex("exe"), "binary-mode sums line");
  assert(sums.get("CineVault_1.2.0_x64_en-US.msi") === hex("msi"), "text-mode sums line");

  const files = [
    { name: "CineVault_1.2.0_x64-setup.exe", sha256: hex("exe"), size: 3 },
    { name: "CineVault_1.2.0_x64_en-US.msi", sha256: hex("msi"), size: 3 },
    { name: "SHA256SUMS-Windows.txt", sha256: hex("sums"), size: 10 },
  ];
  assert(checkAgainstSums(files, sums).length === 0, "matching files pass");
  const tampered = files.map((f) => (f.name.endsWith(".msi") ? { ...f, sha256: hex("evil") } : f));
  assert(checkAgainstSums(tampered, sums).some((p) => p.includes("does not match")), "a changed file is caught");
  const unlisted = [...files, { name: "CineVault_1.2.0_x64-portable.zip", sha256: hex("zip"), size: 3 }];
  assert(checkAgainstSums(unlisted, sums).some((p) => p.includes("not listed")), "an unlisted installer is caught");
  assert(checkAgainstSums(files.slice(1), sums).some((p) => p.includes("not downloaded")), "a missing file is caught");

  const windows = buildPayload({ version: "1.2.0", pubDate: "2026-11-01T10:00:00Z", notes: "n", files });
  assert(Object.keys(windows.platforms).join() === "windows-x86_64", "Windows only: one platform, the setup exe");
  assert(
    windows.platforms["windows-x86_64"].url ===
      "https://github.com/mustafayevt/CineVault-releases/releases/download/v1.2.0/CineVault_1.2.0_x64-setup.exe",
    "public download address",
  );
  assert(windows.platforms["windows-x86_64"].sha256 === hex("exe") && windows.platforms["windows-x86_64"].size === 3, "hash and size");

  const withMac = buildPayload({
    version: "1.2.0",
    pubDate: "x",
    notes: "",
    files: [...files, { name: "CineVault_1.2.0_universal.dmg", sha256: hex("dmg"), size: 4 }],
  });
  assert(
    Object.keys(withMac.platforms).join() === "darwin-aarch64,darwin-x86_64,windows-x86_64",
    "a universal DMG adds both darwin platforms",
  );
  let threw = false;
  try {
    buildPayload({ version: "1.2.0", pubDate: "x", notes: "", files: files.slice(1) });
  } catch {
    threw = true;
  }
  assert(threw, "a release without an updatable installer is refused");

  assert(meaningfulBody("**Full Changelog**: https://x/compare/v0.2.0...v0.2.1") === null, "generated link alone is no notes");
  assert(meaningfulBody("Faster scans.\n\n**Full Changelog**: x") === "Faster scans.", "a written body is kept");
  const notes = notesFromCommits([
    "feat(settings): add the Keep sub-folders toggle",
    "fix(ui): count sizes in 1024s on Windows",
    "chore(release): 0.2.1",
    "test(compat): run fixtures",
    "feat!: drop something",
  ]);
  assert(
    notes === "New\n- Add the Keep sub-folders toggle\n- Drop something\n\nFixes\n- Count sizes in 1024s on Windows",
    `notes grouping, got ${JSON.stringify(notes)}`,
  );
  assert(stableName("CineVault_1.2.0_x64-setup.exe") === "CineVault-setup-x64.exe", "stable setup name");
  assert(stableName("CineVault_1.2.0_x64_en-US.msi") === "CineVault-x64.msi", "stable MSI name");
  assert(stableName("CineVault_1.2.0_x64-portable.zip") === "CineVault-portable-x64.zip", "stable portable name");
  assert(stableName("CineVault_1.2.0_universal.dmg") === "CineVault-universal.dmg", "stable DMG name");
  assert(stableName("SHA256SUMS-Windows.txt") === null && stableName("manifest.json") === null, "no stable name for other files");
  assert(classifyAsset(stableName("CineVault_1.2.0_x64-setup.exe"))?.kind !== "installer", "the stable setup copy is not a second installer");
  assert(previousTag(["v0.1.0", "v0.2.0", "v0.2.1", "v0.3.0", "nightly"], "0.2.1") === "v0.2.0", "previous tag");
  assert(previousTag(["v0.2.1"], "0.2.1") === null, "no previous tag");

  // Linux: the AppImage is the installer, the packages are only uploaded, and no version-less
  // copy is a second installer.
  const linuxFiles = [
    { name: "CineVault_1.2.0_amd64.AppImage", sha256: hex("app"), size: 3 },
    { name: "CineVault_1.2.0_amd64.deb", sha256: hex("deb"), size: 3 },
    { name: "CineVault-1.2.0-1.x86_64.rpm", sha256: hex("rpm"), size: 3 },
  ];
  assert(
    classifyAsset(linuxFiles[0].name)?.platforms?.join() === "linux-x86_64",
    "the AppImage is the linux-x86_64 installer",
  );
  assert(classifyAsset("CineVault_1.2.0_aarch64.AppImage")?.platforms?.join() === "linux-aarch64", "an arm64 AppImage");
  assert(
    classifyAsset(linuxFiles[1].name)?.kind === "upload" && classifyAsset(linuxFiles[2].name)?.kind === "upload",
    "packages are uploaded, not in the manifest",
  );
  assert(classifyAsset("CineVault-x86_64.AppImage") === null, "the version-less AppImage is not an installer");
  assert(stableName("CineVault_1.2.0_amd64.AppImage") === "CineVault-x86_64.AppImage", "stable AppImage name");
  assert(stableName("CineVault_1.2.0_aarch64.AppImage") === "CineVault-aarch64.AppImage", "stable arm64 AppImage name");
  assert(stableName("CineVault_1.2.0_amd64.deb") === "CineVault-amd64.deb", "stable deb name");
  assert(stableName("CineVault-1.2.0-1.x86_64.rpm") === "CineVault-x86_64.rpm", "stable rpm name");
  for (const name of [...files, ...linuxFiles].map((f) => f.name)) {
    const stable = stableName(name);
    assert(
      stable === null || classifyAsset(stable)?.kind !== "installer",
      `the stable copy of ${name} is not a second installer`,
    );
  }
  const everything = buildPayload({
    version: "1.2.0",
    pubDate: "x",
    notes: "",
    files: [...files, ...linuxFiles, { name: "CineVault_1.2.0_universal.dmg", sha256: hex("dmg"), size: 4 }],
  });
  assert(
    Object.keys(everything.platforms).join() === "darwin-aarch64,darwin-x86_64,linux-x86_64,windows-x86_64",
    "platform keys sort with the Linux AppImage in",
  );
  assert(
    everything.platforms["linux-x86_64"].url ===
      "https://github.com/mustafayevt/CineVault-releases/releases/download/v1.2.0/CineVault_1.2.0_amd64.AppImage",
    "AppImage download address",
  );
  assert(
    namesVersion("CineVault_1.2.0_x64-setup.exe", "1.2.0") && namesVersion("CineVault-1.2.0-1.x86_64.rpm", "1.2.0"),
    "a version is found in both naming styles",
  );
  assert(
    !namesVersion("CineVault_1.2.10_x64-setup.exe", "1.2.1") && !namesVersion("CineVault_1.2.0_x64-setup.exe", "1.2.1"),
    "another version is not",
  );

  // Local mode, with fake build folders: an older build and a stale checksum file are left in them.
  const refuses = async (run, pattern) => {
    try {
      await run();
    } catch (error) {
      return pattern.test(error.message);
    }
    return false;
  };
  const root = mkdtempSync(join(tmpdir(), "cinevault-selftest-"));
  try {
    const nsis = join(root, "nsis");
    const linux = join(root, "linux");
    mkdirSync(nsis);
    mkdirSync(linux);
    writeFileSync(join(nsis, "CineVault_1.2.0_x64-setup.exe"), "exe");
    writeFileSync(join(nsis, "CineVault_1.1.0_x64-setup.exe"), "older build");
    writeFileSync(join(nsis, "SHA256SUMS-Windows.txt"), "stale checksums");
    writeFileSync(join(linux, "CineVault_1.2.0_amd64.AppImage"), "app");
    writeFileSync(join(linux, "CineVault_1.2.0_amd64.deb"), "deb");
    writeFileSync(join(linux, "CineVault-1.2.0-1.x86_64.rpm"), "rpm");
    writeFileSync(join(linux, "notes.txt"), "not a release file");
    const found = findLocalFiles([nsis, linux], "1.2.0");
    assert(found.files.length === 4 && found.skipped.join() === "CineVault_1.1.0_x64-setup.exe", "only the version's files are taken");

    const dup = join(root, "dup");
    mkdirSync(dup);
    writeFileSync(join(dup, "CineVault_1.2.0_amd64.deb"), "another copy");
    assert(
      await refuses(() => findLocalFiles([linux, dup], "1.2.0"), /is in two folders/),
      "a file in two folders is refused",
    );
    assert(
      await refuses(() => findLocalFiles([join(root, "missing")], "1.2.0"), /is not a folder/),
      "a missing folder is refused",
    );

    const work = join(root, "work");
    mkdirSync(work);
    assert(
      await refuses(() => fromBuildFolders([nsis], "9.9.9", work), /no CineVault 9\.9\.9 installers/),
      "a folder without that version is refused",
    );
    const local = await fromBuildFolders([nsis, linux], "1.2.0", work);
    const sumsFiles = local.filter((f) => classifyAsset(f.name)?.kind === "sums");
    assert(
      sumsFiles.map((f) => f.name).sort().join() === "SHA256SUMS-Linux.txt,SHA256SUMS-Windows.txt",
      "one checksum file per operating system",
    );
    const localSums = new Map();
    for (const file of sumsFiles) {
      for (const [name, hash] of parseSums(readFileSync(file.path, "utf8"))) localSums.set(name, hash);
    }
    assert(checkAgainstSums(local, localSums).length === 0, "the made checksums match the copies");
    assert(
      localSums.get("CineVault_1.2.0_x64-setup.exe") === hex("exe") &&
        localSums.get("CineVault-1.2.0-1.x86_64.rpm") === hex("rpm"),
      "checksums are of the copied bytes",
    );
    assert(
      readFileSync(join(work, "CineVault_1.2.0_amd64.AppImage"), "utf8") === "app",
      "the installer is copied into the work folder",
    );
    const localPayload = buildPayload({ version: "1.2.0", pubDate: "x", notes: "", files: local });
    assert(
      Object.keys(localPayload.platforms).join() === "linux-x86_64,windows-x86_64",
      "a local build gives the Windows and Linux entries",
    );
  } finally {
    rmSync(root, { recursive: true, force: true });
  }

  // Signing round trip with a throwaway key, and the checks that must fail.
  const { privateKey, publicKey } = generateKeyPairSync("ed25519");
  const publicText = rawPublicKey(publicKey).toString("base64");
  const bytes = Buffer.from(JSON.stringify(windows, null, 2) + "\n");
  const envelope = signPayload(bytes, privateKey);
  assert(JSON.stringify(verifyEnvelope(envelope, publicText)) === JSON.stringify(windows), "signed payload verifies");
  const other = rawPublicKey(generateKeyPairSync("ed25519").publicKey).toString("base64");
  let refused = false;
  try {
    verifyEnvelope(envelope, other);
  } catch {
    refused = true;
  }
  assert(refused, "another key is refused");
  const parsed = JSON.parse(envelope);
  parsed.payload = Buffer.from(bytes.toString().replace("1.2.0", "9.9.9")).toString("base64");
  refused = false;
  try {
    verifyEnvelope(JSON.stringify(parsed), publicText);
  } catch {
    refused = true;
  }
  assert(refused, "a changed payload is refused");
  assert(builtInPublicKey()?.length === 44, "the built-in public key is readable from manifest.rs");
  console.log("release-publish self-test passed");
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.selfTest) await selfTest();
    else await publish(options);
  } catch (error) {
    console.error(`release:publish: ${error.message}`);
    process.exit(1);
  }
}
