// Generates THIRD_PARTY_NOTICES.md from the Rust (cargo-about) and UI (license-checker)
// dependency trees. Run from the repository root: `bun run notices`.
// Needs `cargo install cargo-about --locked --features cli`.
import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const out = join(root, "THIRD_PARTY_NOTICES.md");
const tmp = join(root, "target");
mkdirSync(tmp, { recursive: true });
const env = {
  ...process.env,
  PATH: `${join(homedir(), ".cargo", "bin")}${delimiter}${process.env.PATH ?? ""}`,
};

const rustFile = join(tmp, "rust-notices.md");
execFileSync("cargo", ["about", "generate", "--offline", "about.hbs", "-o", "target/rust-notices.md"], {
  cwd: root,
  env,
  stdio: "inherit",
});
const rust = readFileSync(rustFile, "utf8");

const uiFile = join(tmp, "ui-licenses.json");
// bun may be a .cmd shim on Windows, which needs a shell to launch.
execFileSync("bun run notices", { cwd: join(root, "ui"), env, stdio: "inherit", shell: true });
const ui = JSON.parse(readFileSync(uiFile, "utf8"));

const uiSections = Object.keys(ui)
  .sort((a, b) => a.localeCompare(b))
  .map((id) => {
    const entry = ui[id];
    const licenseFile = entry.licenseFile;
    const isLicenseText = licenseFile && existsSync(licenseFile) && !/readme/i.test(licenseFile);
    const text = isLicenseText ? readFileSync(licenseFile, "utf8").trim() : null;
    const lines = [`### ${id}`, "", `Licence: ${entry.licenses}`];
    if (entry.repository) lines.push(`Repository: ${entry.repository}`);
    if (text) lines.push("", "```text", text, "```");
    return lines.join("\n");
  })
  .join("\n\n");

// Programs CineVault downloads on demand. They are not bundled with the installer or linked into
// the app (MediaInfo is loaded as a separate library), so they are listed from the pinned manifest.
const manifest = JSON.parse(
  readFileSync(join(root, "crates", "cv-media", "src", "tools", "manifest.json"), "utf8"),
);
const toolSections = Object.values(manifest.packages)
  .map((pkg) => {
    const versions = Object.entries(pkg.platforms)
      .map(([platform, spec]) => `${platform} ${spec.version}`)
      .join(", ");
    const hosts = new Set(
      Object.values(pkg.platforms).flatMap((spec) => spec.artifacts.map((a) => new URL(a.url).host)),
    );
    return [
      `### ${pkg.display}`,
      "",
      `Licence: ${pkg.license}`,
      `Homepage: ${pkg.homepage}`,
      `Pinned versions: ${versions}`,
      `Downloaded from: ${[...hosts].join(", ")}`,
    ].join("\n");
  })
  .join("\n\n");
const toolsNote =
  "These programs are not bundled with CineVault. The app downloads them on demand into its own " +
  "data folder, checks each file against a pinned SHA-256, and never installs them system-wide. " +
  "The FFmpeg build for Windows is a GPL build; FFmpeg's own source is licensed under the LGPL " +
  "and GPL. The licence text that comes with each download is kept beside the program.";

const header = `# Third-party notices

CineVault includes the open-source components listed below. This file is generated from the
dependency licences; do not edit it by hand. Regenerate it with \`bun run notices\`.
`;

const body = `${header}\n${rust.trim()}\n\n## UI packages (bun / npm)\n\n${uiSections}\n\n## External tools (downloaded on demand)\n\n${toolsNote}\n\n${toolSections}\n`;
writeFileSync(out, body.replace(/\r\n/g, "\n"));
console.log(`Wrote ${out}`);
