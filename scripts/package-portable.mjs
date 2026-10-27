// Builds the portable Windows zip from the release executable.
//
//   bun run package:portable      (after `bun run tauri build`, or `cargo build --release -p cinevault-app`)
//
// The zip holds CineVault.exe and an empty file named `portable` next to it. The app sees the
// marker (crates/cv-app/src/data_location.rs) and keeps all data in a `data` folder beside the
// executable, so nothing is written to the user profile.
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
if (process.platform !== "win32") {
  console.error("The portable zip is built on Windows only.");
  process.exit(1);
}
const exe = join(root, "target", "release", "cinevault-app.exe");
if (!existsSync(exe)) {
  console.error(`Missing ${exe}. Run "bun run tauri build" first.`);
  process.exit(1);
}
const version = JSON.parse(readFileSync(join(root, "src-tauri", "tauri.conf.json"), "utf8")).version;
const outDir = join(root, "target", "release", "bundle", "portable");
const stage = join(outDir, "CineVault");
rmSync(outDir, { recursive: true, force: true });
mkdirSync(stage, { recursive: true });
copyFileSync(exe, join(stage, "CineVault.exe"));
writeFileSync(join(stage, "portable"), "");
const zip = join(outDir, `CineVault_${version}_x64-portable.zip`);
// bsdtar ships with Windows 10 and later and writes zip files with `-a`. Use it by full path:
// a GNU tar earlier on PATH (Git Bash) cannot read drive letters.
const tar = join(process.env.SystemRoot ?? "C:\Windows", "System32", "tar.exe");
const result = spawnSync(tar, ["-a", "-c", "-f", zip, "-C", stage, "CineVault.exe", "portable"], {
  stdio: "inherit",
});
if (result.status !== 0) process.exit(result.status ?? 1);
console.log(zip);
