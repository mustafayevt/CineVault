// Publishes the website: builds a commit of the `gh-pages` branch from the contents of `site/` in
// a temporary worktree and pushes it. GitHub Pages of the project repository serves that branch at
// https://mustafayevt.github.io/CineVault/. Maintainers only (needs push access over SSH).
//
//   node scripts/site-deploy.mjs                 build the commit and push it
//   node scripts/site-deploy.mjs --dry-run       build the commit, show what changed, push nothing
//   node scripts/site-deploy.mjs --self-test
//
// Options: --remote <name> (default origin), --branch <name> (default gh-pages).
//
// The branch holds only the site: the files of site/ without its README.md, plus an empty
// .nojekyll file. changelog.html is checked against CHANGELOG.md first (bun run site:changelog).
// Nothing in your working tree or on your current branch changes. The first run creates the branch
// without history; later runs add one commit on top of the pushed branch, and nothing is pushed
// when the site did not change. Pages must be switched on once: Settings → Pages → Deploy from a
// branch → gh-pages, / (root), or
//   gh api -X POST repos/mustafayevt/CineVault/pages -f "source[branch]=gh-pages" -f "source[path]=/"
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");

/** Files of site/ that are not part of the website. */
export const NOT_PUBLISHED = new Set(["README.md"]);

/** The names in site/ that go to the branch. */
export function publishedNames(names) {
  return names.filter((name) => !NOT_PUBLISHED.has(name)).sort();
}

function parseArgs(argv) {
  const options = { dryRun: false, selfTest: false, remote: "origin", branch: "gh-pages" };
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    const value = () => {
      const next = argv[++i];
      if (next === undefined) throw new Error(`${arg} needs a value`);
      return next;
    };
    if (arg === "--dry-run") options.dryRun = true;
    else if (arg === "--self-test") options.selfTest = true;
    else if (arg === "--remote") options.remote = value();
    else if (arg === "--branch") options.branch = value();
    else throw new Error(`unknown option ${arg}`);
  }
  return options;
}

const git = (args, cwd = root) =>
  execFileSync("git", args, { cwd, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"], maxBuffer: 64 * 1024 * 1024 }).trim();

function deploy(options) {
  const { remote, branch } = options;
  // The changelog page must be what CHANGELOG.md gives.
  execFileSync(process.execPath, [join(root, "scripts", "site-changelog.mjs"), "--check"], { cwd: root, stdio: "inherit" });

  const source = git(["rev-parse", "--short", "HEAD"]);
  const names = publishedNames(readdirSync(join(root, "site")));
  if (!names.includes("index.html")) throw new Error("site/index.html is missing");

  // The branch as it is on the remote now (none the first time).
  let base = null;
  try {
    git(["fetch", remote, `${branch}:refs/remotes/${remote}/${branch}`, "--force"]);
    base = `${remote}/${branch}`;
  } catch {
    console.log(`No ${branch} on ${remote} yet: the first deploy creates it.`);
  }

  // A private local branch name, so a gh-pages branch of your own is never touched.
  const local = `site-deploy-${process.pid}`;
  const work = mkdtempSync(join(tmpdir(), "cinevault-site-"));
  const tree = join(work, "tree");
  try {
    git(["worktree", "add", "--detach", tree, base ?? "HEAD"]);
    if (base) {
      git(["checkout", "-B", local, base], tree);
    } else {
      git(["checkout", "--orphan", local], tree);
    }
    // Start from an empty tree, then copy the site in.
    for (const entry of readdirSync(tree)) {
      if (entry !== ".git") rmSync(join(tree, entry), { recursive: true, force: true });
    }
    for (const name of names) cpSync(join(root, "site", name), join(tree, name), { recursive: true });
    writeFileSync(join(tree, ".nojekyll"), "");
    git(["add", "-A"], tree);

    const changed = git(["status", "--porcelain"], tree);
    if (changed === "") {
      console.log(`${branch} on ${remote} already has this site. Nothing to deploy.`);
      return;
    }
    console.log(`Changes against ${base ?? "an empty branch"}:`);
    console.log(git(["diff", "--cached", "--stat", "--stat-count=30"], tree).replace(/^/gm, "  "));
    git(["commit", "-m", `chore(site): deploy the site from ${source}\n\nBuilt from site/ at commit ${source} of the main branch.`], tree);
    if (options.dryRun) {
      console.log(`\nDry run: built commit ${git(["rev-parse", "--short", "HEAD"], tree)} for ${branch}; nothing was pushed.`);
      return;
    }
    git(["push", remote, `${local}:${branch}`], tree);
    console.log(`\nPushed ${branch} to ${remote}. Pages rebuilds within a minute or two:`);
    console.log("  https://mustafayevt.github.io/CineVault/");
  } finally {
    try {
      git(["worktree", "remove", "--force", tree]);
    } catch {
      /* the folder is removed below */
    }
    rmSync(work, { recursive: true, force: true });
    try {
      git(["worktree", "prune"]);
      git(["branch", "-D", local]);
    } catch {
      /* no local branch was made */
    }
  }
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  assert(
    publishedNames(["style.css", "README.md", "index.html", "img"]).join() === "img,index.html,style.css",
    "the README is not published, the rest is, sorted",
  );
  for (const name of ["index.html", "changelog.html", "site.js", "style.css", "img"]) {
    assert(existsSync(join(root, "site", name)), `site/${name} exists`);
  }
  const options = parseArgs(["--dry-run", "--branch", "pages"]);
  assert(options.dryRun && options.branch === "pages" && options.remote === "origin", "options");
  console.log("site-deploy self-test passed");
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  try {
    const options = parseArgs(process.argv.slice(2));
    if (options.selfTest) selfTest();
    else deploy(options);
  } catch (error) {
    const detail = error.stderr ? String(error.stderr).trim() : error.message;
    console.error(`site:deploy: ${detail}`);
    process.exit(1);
  }
}
