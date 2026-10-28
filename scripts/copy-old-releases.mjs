// Copies the earlier releases (v0.2.2 to v1.0.1) from the earlier releases repository to the
// project repository, so the project's Releases page shows the whole history. Maintainers only:
// it needs the `gh` CLI signed in with write access to the project repository.
//
//   node scripts/copy-old-releases.mjs             shows what would be copied (dry run)
//   node scripts/copy-old-releases.mjs --apply     creates the releases
//   node scripts/copy-old-releases.mjs --self-test
//
// Options: --from <owner/name> (default mustafayevt/CineVault-releases), --to <owner/name>
// (default mustafayevt/CineVault), --commit <sha> (the commit the tags point at; default the first
// commit, d3b54136), --first <x.y.z> (default 0.2.2), --last <x.y.z> (default 1.0.1).
//
// Each release gets the same title, the same assets (downloaded and uploaded again byte for byte,
// the signed manifest.json included, nothing is re-signed) and the same notes, with this line
// added at the top: "Built from the earlier private development history; originally published in
// CineVault-releases." The tags point at --commit, and none of the copies is marked latest (the
// newest release of the project stays the one the update channel reads). A release that already
// exists in the project repository is left alone, so the script can be run again after an
// interruption. The copies start with zero downloads.
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const FROM_REPO = "mustafayevt/CineVault-releases";
export const TO_REPO = "mustafayevt/CineVault";
export const FIRST_COMMIT = "d3b54136";
export const NOTE_LINE =
  "Built from the earlier private development history; originally published in CineVault-releases.";

const parts = (tag) => {
  const match = /^v?(\d+)\.(\d+)\.(\d+)$/.exec(tag);
  return match ? match.slice(1, 4).map(Number) : null;
};
const compare = (a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2];

/** The plain release tags from `first` to `last` (both included), oldest first. */
export function tagsToCopy(tags, first, last) {
  const low = parts(first);
  const high = parts(last);
  return tags
    .map((tag) => ({ tag, p: parts(tag) }))
    .filter(({ p }) => p && compare(p, low) >= 0 && compare(p, high) <= 0)
    .sort((a, b) => compare(a.p, b.p))
    .map(({ tag }) => tag);
}

/**
 * The notes of the copy: the note line, a blank line, then the original notes. A "Moved" line
 * added to the original (the earlier repository points people to the project repository) is not
 * carried over, since the copy lives there.
 */
export function copiedNotes(body) {
  const lines = String(body ?? "").replace(/\r\n/g, "\n").split("\n");
  while (lines.length > 0 && (lines[0].trim() === "" || /^\W*moved\b/i.test(lines[0].trim()))) lines.shift();
  const original = lines.join("\n").trim();
  return original === "" ? `${NOTE_LINE}\n` : `${NOTE_LINE}\n\n${original}\n`;
}

const gh = (args, opts = {}) =>
  execFileSync("gh", args, { encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024, ...opts });

function parseArgs(argv) {
  const options = { apply: false, selfTest: false, from: FROM_REPO, to: TO_REPO, commit: FIRST_COMMIT, first: "0.2.2", last: "1.0.1" };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      return next;
    };
    if (arg === "--apply") options.apply = true;
    else if (arg === "--self-test") options.selfTest = true;
    else if (arg === "--from") options.from = value();
    else if (arg === "--to") options.to = value();
    else if (arg === "--commit") options.commit = value();
    else if (arg === "--first") options.first = value().replace(/^v/, "");
    else if (arg === "--last") options.last = value().replace(/^v/, "");
    else throw new Error(`unknown option ${arg}`);
  }
  return options;
}

const exists = (repo, tag) => {
  try {
    gh(["release", "view", tag, "-R", repo, "--json", "tagName"]);
    return true;
  } catch {
    return false;
  }
};

async function run(options) {
  const { from, to } = options;
  const listed = JSON.parse(gh(["release", "list", "-R", from, "--limit", "200", "--json", "tagName"]));
  const tags = tagsToCopy(listed.map((release) => release.tagName), options.first, options.last);
  if (tags.length === 0) throw new Error(`no releases from v${options.first} to v${options.last} in ${from}`);
  // The full commit id, found in the project repository (the tags need a commit that is there).
  const sha = gh(["api", `repos/${to}/commits/${options.commit}`, "--jq", ".sha"]).trim();
  console.log(`${options.apply ? "Copying" : "Would copy (dry run, nothing is created)"} ${tags.length} releases from ${from} to ${to}`);
  console.log(`Tags point at ${sha}; none of the copies is marked latest.\n`);

  const work = options.apply ? mkdtempSync(join(tmpdir(), "cinevault-copy-releases-")) : null;
  try {
    for (const tag of tags) {
      const release = JSON.parse(gh(["release", "view", tag, "-R", from, "--json", "name,body,assets,isPrerelease"]));
      const size = release.assets.reduce((sum, asset) => sum + asset.size, 0);
      const line = `${tag}  "${release.name}"  ${release.assets.length} files, ${(size / 1024 / 1024).toFixed(1)} MB`;
      if (exists(to, tag)) {
        console.log(`${line}\n  already in ${to}: left alone`);
        continue;
      }
      console.log(line);
      for (const asset of release.assets) console.log(`  ${asset.name}  ${asset.size} bytes`);
      if (!options.apply) continue;

      const dir = join(work, tag);
      mkdirSync(dir);
      gh(["release", "download", tag, "-R", from, "-D", dir], { stdio: ["ignore", "pipe", "inherit"] });
      const notesPath = join(work, `${tag}.md`);
      writeFileSync(notesPath, copiedNotes(release.body));
      // A draft first, so the release is complete before anyone can see it.
      gh(
        [
          "release", "create", tag, "-R", to,
          "--title", release.name,
          "--notes-file", notesPath,
          // The tag is pushed over git first (creating it through the API needs the token's
          // "workflow" scope when the commit holds workflow files), so the release only uses it.
          "--verify-tag",
          "--draft",
          "--latest=false",
          ...(release.isPrerelease ? ["--prerelease"] : []),
        ],
      );
      const files = readdirSync(dir).map((name) => join(dir, name));
      gh(["release", "upload", tag, "-R", to, ...files], { stdio: ["ignore", "inherit", "inherit"] });
      gh(["release", "edit", tag, "-R", to, "--draft=false", "--latest=false"]);
      rmSync(dir, { recursive: true, force: true });
      console.log(`  created ${tag} in ${to}`);
    }
  } finally {
    if (work) rmSync(work, { recursive: true, force: true });
  }
  console.log(options.apply ? "\nDone." : "\nDry run only. Run again with --apply to create them.");
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const tags = ["v1.0.1", "v0.2.2", "v0.2.10", "v0.2.3", "v0.1.0", "v1.1.0", "nightly", "v1.0.0"];
  assert(
    tagsToCopy(tags, "0.2.2", "1.0.1").join() === "v0.2.2,v0.2.3,v0.2.10,v1.0.0,v1.0.1",
    "the range is oldest first, compared as numbers, without other tags",
  );
  assert(copiedNotes("### Added\n- A.\n").startsWith(`${NOTE_LINE}\n\n### Added`), "the note line comes first");
  assert(copiedNotes("").trim() === NOTE_LINE, "an empty body gives the note line alone");
  assert(
    copiedNotes("Moved: download from the project repository\r\n\r\n- A.").trim() === `${NOTE_LINE}\n\n- A.`,
    "a Moved line of the earlier repository is dropped",
  );
  assert(copiedNotes("- A moved folder is kept.").includes("- A moved folder is kept."), "other lines are kept");
  const options = parseArgs(["--apply", "--first", "v0.2.2"]);
  assert(options.apply && options.first === "0.2.2" && options.to === TO_REPO, "options");
  console.log("copy-old-releases self-test passed");
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.selfTest) selfTest();
    else await run(options);
  } catch (error) {
    console.error(`copy-old-releases: ${error.message}`);
    process.exit(1);
  }
}
