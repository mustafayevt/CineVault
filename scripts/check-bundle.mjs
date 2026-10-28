// Checks the Tauri bundling set-up.
//
//   node scripts/check-bundle.mjs               static checks on the configuration
//   node scripts/check-bundle.mjs --artifacts   also require the installers of this OS to exist
//
// The static checks need no build. `--artifacts` runs after `bun run tauri build` and prints
// the size of every installer it finds.
import { existsSync, readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const tauriDir = join(root, "src-tauri");
const problems = [];
const fail = (message) => problems.push(message);

const readJson = (path) => JSON.parse(readFileSync(path, "utf8"));
const base = readJson(join(tauriDir, "tauri.conf.json"));
const windows = readJson(join(tauriDir, "tauri.windows.conf.json"));
const macos = readJson(join(tauriDir, "tauri.macos.conf.json"));
const linux = readJson(join(tauriDir, "tauri.linux.conf.json"));

// 1. Bundling is on and each OS asks for the installers the spec names.
if (base.bundle?.active !== true) fail("bundle.active must be true");
const need = (conf, name, targets) => {
  const list = conf.bundle?.targets;
  if (!Array.isArray(list)) return fail(`${name}: bundle.targets must be a list`);
  for (const t of targets) if (!list.includes(t)) fail(`${name}: bundle.targets lacks "${t}"`);
};
need(windows, "tauri.windows.conf.json", ["msi", "nsis"]);
need(macos, "tauri.macos.conf.json", ["dmg"]);
need(linux, "tauri.linux.conf.json", ["appimage", "deb", "rpm"]);

// 2. Every icon the bundle lists exists, and the icon set covers each platform format.
const icons = base.bundle?.icon ?? [];
for (const icon of icons) {
  if (!existsSync(join(tauriDir, icon))) fail(`missing icon file ${icon}`);
}
for (const ext of [".ico", ".icns", ".png"]) {
  if (!icons.some((i) => i.endsWith(ext))) fail(`bundle.icon has no ${ext} file`);
}

// 2b. The installer images each OS names exist at the size the installer expects (brand-identity).
const imageSize = (path) => {
  const data = readFileSync(path);
  if (data.toString("ascii", 0, 2) === "BM") {
    return { kind: "bmp", width: data.readInt32LE(18), height: Math.abs(data.readInt32LE(22)), bits: data.readUInt16LE(28) };
  }
  if (data.toString("ascii", 1, 4) === "PNG") {
    return { kind: "png", width: data.readUInt32BE(16), height: data.readUInt32BE(20) };
  }
  return { kind: "unknown", width: 0, height: 0 };
};
const installerImages = [
  [windows.bundle?.windows?.nsis?.headerImage, "bmp", 150, 57, "nsis.headerImage"],
  [windows.bundle?.windows?.nsis?.sidebarImage, "bmp", 164, 314, "nsis.sidebarImage"],
  [windows.bundle?.windows?.wix?.bannerPath, "bmp", 493, 58, "wix.bannerPath"],
  [windows.bundle?.windows?.wix?.dialogImagePath, "bmp", 493, 312, "wix.dialogImagePath"],
  [macos.bundle?.macOS?.dmg?.background, "png", 660, 400, "macOS.dmg.background"],
];
for (const [path, kind, width, height, key] of installerImages) {
  if (!path) {
    fail(`${key} is not set`);
    continue;
  }
  const file = join(tauriDir, path);
  if (!existsSync(file)) {
    fail(`${key} names ${path}, which does not exist`);
    continue;
  }
  const size = imageSize(file);
  if (size.kind !== kind) fail(`${path} must be a ${kind} file`);
  else if (size.width !== width || size.height !== height) {
    fail(`${path} is ${size.width} x ${size.height}, expected ${width} x ${height}`);
  } else if (kind === "bmp" && size.bits !== 24) fail(`${path} must be a 24-bit bitmap`);
}
const nsisIcon = windows.bundle?.windows?.nsis?.installerIcon;
if (nsisIcon && !existsSync(join(tauriDir, nsisIcon))) fail(`nsis.installerIcon names ${nsisIcon}, which does not exist`);
const nsisHooks = windows.bundle?.windows?.nsis?.installerHooks;
if (!nsisHooks) fail("nsis.installerHooks must name installer/hooks.nsh (it refreshes pinned icons after an update)");
else if (!existsSync(join(tauriDir, nsisHooks))) fail(`nsis.installerHooks names ${nsisHooks}, which does not exist`);

// 3. Nothing secret or machine-specific goes into the build: no bundled resources or sidecars,
//    no updater key, no signing identity or certificate in the committed configuration.
if (base.bundle?.resources) fail("bundle.resources must be empty (nothing is shipped but the app)");
if (base.bundle?.externalBin) fail("bundle.externalBin must be empty (tools are downloaded on demand)");
if (base.plugins?.updater) fail("the Tauri updater plugin is not used (updates use the signed manifest)");
const configText = [base, windows, macos, linux].map((c) => JSON.stringify(c)).join("\n");
for (const key of ["signingIdentity", "certificateThumbprint", "providerShortName", "pubkey"]) {
  if (configText.includes(`"${key}"`)) fail(`configuration must not set ${key}; signing is a release-time step`);
}

// 4. The workflows use no repository secrets and no API-key variables (the built-in
//    `github.token` passed to the gh CLI as GH_TOKEN is allowed). The one exception is the
//    update-signing key, `secrets.CINEVAULT_UPDATE_KEY`, which only release.yml may read.
const workflowDir = join(root, ".github", "workflows");
for (const file of existsSync(workflowDir) ? readdirSync(workflowDir) : []) {
  const text = readFileSync(join(workflowDir, file), "utf8");
  const allowedSecrets = file === "release.yml" ? ["GITHUB_TOKEN", "CINEVAULT_UPDATE_KEY"] : ["GITHUB_TOKEN"];
  for (const [, name] of text.matchAll(/\$\{\{\s*secrets\.(\w+)/g)) {
    if (!allowedSecrets.includes(name)) fail(`${file} reads the repository secret ${name}`);
  }
  const vars = text.replace(/GITHUB_TOKEN/g, "").replace(/GH_TOKEN:\s*\$\{\{\s*github\.token\s*\}\}/g, "");
  if (/(API_KEY|APP_KEY|_TOKEN|PRIVATE_KEY|PASSWORD)\s*:/.test(vars)) {
    fail(`${file} sets a key, token or password variable`);
  }
}
// The three workflows the project has: checks on every push, the tag-driven release, the site.
for (const file of ["ci.yml", "release.yml", "site.yml"]) {
  if (!existsSync(join(workflowDir, file))) fail(`missing .github/workflows/${file}`);
}

// 5. Version agreement between the package, the Tauri config and the Cargo workspace.
const cargo = readFileSync(join(root, "Cargo.toml"), "utf8");
const cargoVersion = /^version\s*=\s*"([^"]+)"/m.exec(cargo.slice(cargo.indexOf("[workspace.package]")))?.[1];
if (cargoVersion !== base.version) {
  fail(`version mismatch: Cargo workspace ${cargoVersion}, tauri.conf.json ${base.version}`);
}

// 6. Optional: the installers of this OS exist after a build.
const found = [];
if (process.argv.includes("--artifacts")) {
  // The macOS build is universal (Apple Silicon and Intel), which Tauri writes under its own target folder.
  const bundle = [
    join(root, "target", "universal-apple-darwin", "release", "bundle"),
    join(root, "target", "release", "bundle"),
  ].find((dir) => process.platform === "darwin" && existsSync(dir)) ?? join(root, "target", "release", "bundle");
  const wanted =
    process.platform === "win32"
      ? [["msi", ".msi"], ["nsis", ".exe"]]
      : process.platform === "darwin"
        ? [["dmg", ".dmg"]]
        : process.platform === "linux"
          ? [["appimage", ".AppImage"], ["deb", ".deb"], ["rpm", ".rpm"]]
          : [];
  for (const [folder, ext] of wanted) {
    const dir = join(bundle, folder);
    const files = existsSync(dir) ? readdirSync(dir).filter((f) => f.endsWith(ext)) : [];
    if (files.length === 0) fail(`no ${ext} installer in ${dir}`);
    for (const f of files) found.push(`${join(folder, f)}  ${(statSync(join(dir, f)).size / 1048576).toFixed(1)} MiB`);
  }
}

if (problems.length > 0) {
  console.error(`bundle check failed:\n${problems.map((p) => `  - ${p}`).join("\n")}`);
  process.exit(1);
}
for (const line of found) console.log(line);
console.log("bundle check passed");
