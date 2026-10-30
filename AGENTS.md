# CineVault: guide for contributors and AI coding assistants

CineVault is a fast desktop movie and TV library manager for Windows, Linux (beta) and macOS. It scans media folders, identifies titles from online providers, writes NFO and artwork files in the Kodi, Jellyfin and Emby formats, renames folders and files from templates with a preview and Undo, and offers drive moves, torrent-search hand-off, collections and a full change History. It is built for fresh users; there are no importers from other apps.

Human contributors: read [CONTRIBUTING.md](CONTRIBUTING.md) too. `CLAUDE.md` points here.

## Workflow rules (read first)

1. **Never push to `main`.** Start every change on a short-lived branch from `main` (`feat/<topic>`, `fix/<topic>`, `docs/<topic>`, `chore/<topic>`).
2. **Open a pull request** for it (`gh pr create`), describing what changed, why, and how it was checked. CI must pass; then it is squash-merged with a Conventional Commit title.
3. **Behaviour changes go through OpenSpec** (proposal → apply → archive, see below); bug fixes that keep specified behaviour don't need a change folder.
4. **UI changes also update** the screenshots (`.github/assets/`, `site/img/`), the landing page wording, the README and `CHANGELOG.md` (then `bun run site:changelog`) in the same pull request.
5. **Commits:** Conventional Commits, a short body saying what and why, and no AI attribution of any kind (no `Co-Authored-By`, no "Generated with" footers).
6. **Releases** are cut from `main` by pushing a `v<version>` tag after the version bump has been merged; never by pushing to `main`.

## Source of truth

- **Behaviour:** `openspec/specs/<capability>/spec.md` describes what each capability does today. Specs win over issue or task wording. Proposed work lives in `openspec/changes/<name>/` (`proposal.md`, `design.md`, delta `specs/<capability>/spec.md`, `tasks.md`).
- **UI:** the `ui-design-system` spec, `ui/src/styles/tokens.css` (tokens), `ui/src/styles/design-tokens.json` (its machine-readable twin; `tokens.test.ts` checks they agree) and the shared components in `ui/src/components`, shown on the `/gallery` route of the dev build.
- **Expected file output:** the golden files in `tests/compat/` (NFO, renamer and name-parser results, byte for byte).
- **Project rules for OpenSpec:** `openspec/config.yaml`.

## Layout

```
crates/cv-domain     shared types (titles, files, editions, ids)
crates/cv-fs         safe writes into media folders (flushed atomic replace, short retries
                     for files another program holds)
crates/cv-db         SQLite storage: schema, migrations, repositories, the forgiving library search
crates/cv-scan       folder walking, name parsing (title, year, episode, ids), stacking
crates/cv-media      MediaInfo, FFmpeg and external tools (downloaded on demand, checksummed)
crates/cv-template   the `${token}` naming templates
crates/cv-nfo        NFO writers and tolerant readers
crates/cv-providers  metadata, artwork and rating providers (TMDB, TVDB, OMDb, MDBList, Trakt,
                     anime and others) with shared HTTP, cache and rate limiting
crates/cv-match      match confidence scoring, evidence and the review queue
crates/cv-artwork    artwork selection, download, naming and the thumbnail cache
crates/cv-rename     the rename planner, executor and journal (undoable)
crates/cv-jobs       background job lanes, progress, cancel and persistence
crates/cv-torrent    torrent search sources and release ranking
crates/cv-app        the application services the desktop app and CLI share
crates/cv-compat     the test runner over tests/compat/ (not shipped)
src-tauri/           Tauri 2 desktop app (commands, channels, cv:// image protocol, window chrome)
cli/                 the `cinevault` command-line binary
ui/                  React 19 + TypeScript + Vite + Tailwind v4 frontend
tests/compat/        golden fixtures and expected outputs (byte-exact NFO, renamer, parser checks)
tests/nfo-read/      hand-written NFOs (some deliberately broken) for the tolerant readers
assets/brand/        logo, app icons, installer images and glyphs
site/                the public landing page (https://mustafayevt.github.io/CineVault/, deployed to GitHub Pages by the Deploy site workflow)
site-redirect/       the one-page redirect for the old website address in CineVault-releases
scripts/             build, packaging, licence-notice, performance and release scripts
openspec/            specs and change proposals
```

## Commands

**Rust** (on Windows Git Bash, first `export PATH="$HOME/.cargo/bin:$PATH"`):

- `cargo fmt --all`
- `cargo clippy --workspace --all-targets -- -D warnings` (alias: `cargo lint`)
- `cargo test --workspace` (or `-p <crate>` for one crate)
- golden files: `cargo test -p cv-compat` (add `-- --nocapture` for the report), `cargo run -p cv-compat --bin compat-report -- --all` for the per-fixture list; after an intended output change, `CINEVAULT_UPDATE_COMPAT=1 cargo test -p cv-compat` rewrites the expected files, and the diff must be reviewed before committing (see `tests/compat/README.md`)
- CLI: `cargo run -p cinevault-cli -- movie --help` (the program is `cinevault`)

**UI** (inside `ui/`, with bun):

- `bun install`, `bun run dev`, `bun run build`
- `bun run lint`, `bun run typecheck`, `bun run test`, `bun run format` / `bun run format:check` (Prettier)
- `bun run shots -- --routes /,/movies,/gallery`: screenshots of each route in dark and light at 1440×900 and 1366×768 in headless Chrome with a throwaway profile, against the synthetic backend; writes PNGs to the git-ignored `ui/.shots/` and fails on console errors or horizontal overflow. Options: `--all` (every route), `--themes`, `--sizes`, `--url`, `--out`, `--full`, `--scroll <selector>`, `--steps '<selector>|<selector>'` (with `focus:`, `key:` and `eval:` steps), `--reduced`, `--scale <ratio>`, `--visible '<selector>,…'`.
- `bun run check:facts`: the Movie page's first screen at 1366×768 and 1440×900 in both themes must show the plot, the director and release facts and the Technical summary rows without scrolling. Run it after changing the Movie page header, facts or Technical card.
- `bun run perf:grid`: poster-grid scrolling trace in headless Chrome (writes to `target/perf/`).

**Repository root:**

- `bun install`, then `bun run tauri dev` (the desktop app) or `bun run tauri build` (MSI and NSIS installers on Windows, DMG and `.app` on macOS, under `target/release/bundle/`)
- `bun run server:dev` builds the UI and starts `cinevault-server` on the loopback address; `bun run server:build` builds the release binary; `bun run server:image` builds the container image; `bun run server:package` makes the server `.deb` and `.rpm` on Linux; `bun run server:tarball` makes the tarball; `bun run check:scripts` checks that the `bun run` commands named in this file and the README exist
- `bun run tools:fetch`: downloads the pinned MediaInfo, FFmpeg and helper tools into the git-ignored `.dev-tools/` (`-- ffmpeg` fetches one, `-- --help` lists them)
- `bun run bundle:check` (add `-- --artifacts` after a build to require the installers), `bun run package:portable` (portable Windows zip)
- `bun run notices`: regenerates `THIRD_PARTY_NOTICES.md` (needs `cargo install cargo-about --locked --features cli`)
- `bun run perf:budgets`: the performance probes in the release profile on a quiet machine (needs `bun run build` in `ui/` first); `bun run perf:coldstart` times the real release window on a 5,000-movie portable install (Windows)
- `bun run site:changelog`: regenerates `site/changelog.html` from `CHANGELOG.md` (run it after every changelog edit; `-- --check` fails when the page is stale). `node scripts/changelog.mjs --self-test` checks the changelog reader
- The website is deployed by `.github/workflows/site.yml` (**Deploy site**) on every push to `main` that changes `site/**` or `CHANGELOG.md`: it checks `bun run site:changelog -- --check`, then publishes `site/` (without its README) to GitHub Pages at `https://mustafayevt.github.io/CineVault/`. Rerun it by hand with `gh workflow run site.yml` (steps in `site/README.md`). There is no local deploy script.
- `bun run releases:copy-old`: maintainers only, once. Recreates v0.2.2 to v1.0.1 from the earlier releases repository (`mustafayevt/CineVault-releases`) as releases of this repository, with the same assets and notes plus a first line saying where they came from, tags at the first commit and none marked latest. Dry run unless `-- --apply`.
- `bun run release:keys` and `bun run release:publish`: maintainers only (update signing and publishing; the private key never enters the repository). The release goes to this repository (`mustafayevt/CineVault`, tagged `v<version>` at HEAD, which must be pushed) and to the earlier releases repository (`mustafayevt/CineVault-releases`) where installed 1.0.x copies still look; `-- --repo <owner/name>` (repeatable) chooses the repositories instead, and the manifest's installer addresses point at the first one. The app reads this repository's channel first and the earlier one only when that cannot be reached. `bun run release:publish -- --from-dir <folder>` publishes installers built on this machine, with one `--from-dir` per build folder; `-- --dry-run` uploads nothing, `-- --self-test` checks the logic offline. `-- --unsigned` (with `--from-dir`; what the release workflow runs without a signing key) uploads the installers and checksums as a pre-release with no manifest, to be signed later with `bun run release:publish -- --version X` (no `--from-dir`: it reads that release's assets). `node scripts/check-version.mjs [X]` checks that the four version files agree (and say X). The notes of version X are the `## [X]` section of `CHANGELOG.md`

**IPC bindings:** `ui/src/bindings.ts` is generated by `tauri-specta` (on every debug app start, or `CINEVAULT_UPDATE_BINDINGS=1 cargo test -p cinevault-app`). Commit it; the `generated_bindings_are_up_to_date` test fails when it is stale.

**OpenSpec:** `openspec instructions apply --change <name> --json` for progress and tasks; `openspec validate <name> --strict` after editing a change (`openspec validate --specs --strict` for the main specs).

Keep this list accurate: when you add or rename a script, update this file in the same commit.

## Conventions

**Rust**

- Edition 2024; `unsafe` is denied. The one exception is the MediaInfo FFI module in `cv-media`, which uses a scoped `#[allow(unsafe_code)]` with a `SAFETY:` comment on every unsafe block.
- `thiserror` for library errors, `anyhow` only in binaries.
- `tracing` for logs; never log a secret (API keys, tokens).
- Every public behaviour has tests next to it or in `tests/`.

**UI**

- Functional components and hooks with explicit types (no `any`).
- Colours, radii, spacing, shadows, durations and easing come from the design tokens as CSS variables. No hard-coded hex, except rating-source brand colours.
- Icons from `lucide-react` at 1.75 stroke.
- Every control is keyboard reachable with a visible focus ring; icon-only buttons have an `aria-label`.
- Pickers are pages (Scrape match, Artwork, Rename preview, Edit). The only dialogs are the delete and permanent-delete confirmations; what floats is menus, tooltips, popovers, toasts and the selection bar. There are no single-key shortcuts: Ctrl/Alt chords, Delete, Shift+Delete, Esc, F2, Enter and the arrow keys only. Esc closes the innermost menu, dialog or picker page, then clears the selection. Reversible actions confirm with an Undo toast.
- One verb per job everywhere (UI, CLI, docs): Scan, Scrape, Rename. A title's stage is New → Scraped → Renamed. Scrape is the only verb for fetching metadata and choosing a match. `ui/src/i18n/verbs.test.ts` fails when a message uses a retired verb.
- Optional features are off by default and switched on in Settings › Features. While off, their UI and background work are hidden and stopped.
- Every change that alters the UI also refreshes the screenshots in the same piece of work: the README images in `.github/assets/` and the landing-page images in `site/img/` (dark and light pairs, via `bun run shots`), and updates the landing page wording, the changelog (`CHANGELOG.md` plus the regenerated `site/changelog.html`) and the README where the change is visible.
- `CHANGELOG.md` (Keep a Changelog layout, plain user-facing words, newest first) is bundled into the app: the "What's new" card after an update and Settings › Updates › Release notes read it. Add a line under the unreleased version for every user-visible change.

**IPC**

- Tauri commands and events are typed with `tauri-specta`. Long work streams over Tauri channels.
- Never send images over IPC; use the `cv://img` protocol.

**Data safety** (the user's media files come first)

- Never delete library rows automatically.
- Write settings and media-folder files atomically (`cv-fs`).
- Renames go through the planner → preview → journal, so every rename can be undone.
- Delete follows Settings › Deleted files: the Recycle Bin (the default), a trash folder on each drive (`.deletedByCineVault`), a custom trash folder, or permanent deletion. Rename leftovers and replaced artwork or NFO files go to a trash folder, never straight to permanent deletion. Automatic emptying only ever touches CineVault's own trash folders, never the system bin.
- Delete asks for confirmation before anything moves. Shift+Delete is always a permanent delete and needs the typed confirmation, whatever the setting.
- Moves to another drive copy, verify, and only then send the originals away.

**Naming**

- Describe compatibility as the Kodi, Jellyfin, Emby or MediaPortal formats. Don't name other media-manager apps in code, comments, UI text, tests, docs or commit messages.

**Testing**

- Tests never create large files (NTFS has no sparse files, so a "big" test file really takes its size on disk). Use temporary directories and clean them up.
- Tests that call live services are opt-in through an environment variable and skip without it, using the contributor's own keys (for example `CINEVAULT_LIVE_TOOLS=1 cargo test -p cv-media --test tools_live`). The build and the repository hold no API keys; users enter their own, and the app keeps them in the OS keyring.
- For UI screenshots use headless Chrome with a throwaway profile (`--headless=new --user-data-dir=<temp dir> --no-first-run --disable-extensions`), never a personal browser profile, and stop only the processes you started.
- Rust build folders grow quickly; delete `target/` when you no longer need it.

## Workflow

- **New features and behaviour changes** go through OpenSpec: agree on the idea in an issue, then propose a change (`/opsx:propose` or `openspec` by hand: proposal, design, delta specs, tasks), implement it task by task (`/opsx:apply`), and archive it once merged (`/opsx:archive`), which folds its delta specs into `openspec/specs/`. Bug fixes that do not change specified behaviour need no change folder. UI redesigns start with a mock-up for discussion before code.
- **Commits** follow [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/): `<type>(<scope>): <description>` with types `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `build`, `ci`, `perf`, `style`; `!` or a `BREAKING CHANGE:` footer for breaking changes. One logical change per commit.
- **Branches and pull requests:** every change, including the maintainers', goes through a pull request from a short-lived branch (`feat/<topic>`, `fix/<topic>`, `docs/<topic>`); nobody pushes to `main` directly. CI must pass before merging, and pull requests are squash-merged with a Conventional Commit title. Releases are cut from `main` by pushing a `v<version>` tag.
- **Before a pull request**, run the Rust and UI checks above. CI (`.github/workflows/ci.yml`) runs them on every pull request and every push to `main`, on Windows and Linux (format, clippy, tests, compatibility fixtures, UI lint, typecheck, format, tests and build, `node scripts/check-bundle.mjs`, the script self-tests, `bun run site:changelog -- --check`, and the OpenSpec specs). Pushes to `main` also build the installers and keep them as workflow artifacts for 7 days. Both build jobs (`Build and test (windows-latest)` and `Build and test (ubuntu-22.04)`) must pass before a pull request can be merged. Dependency updates arrive weekly from Dependabot (`.github/dependabot.yml`).
- **Releases** are tag-driven (`.github/workflows/release.yml`). Bump the version together in `src-tauri/tauri.conf.json`, `package.json`, `ui/package.json` and the root `Cargo.toml` in a pull request, and have the screenshots (`.github/assets/`, `site/img/`), the landing page, the README and the changelog current in it: the version's section in `CHANGELOG.md` has its release date instead of "unreleased" and `bun run site:changelog` has been run (its text is what the release and the update manifest carry). After the merge: `git switch main && git pull && git tag vX.Y.Z && git push origin vX.Y.Z`. The workflow checks the tag against the version files, builds the Windows (NSIS, MSI, portable zip) and Linux (AppImage, deb, rpm) installers in parallel and publishes the release of `mustafayevt/CineVault` with them, the checksums, the notes and the signed update manifest. It signs with the repository secret `CINEVAULT_UPDATE_KEY` (the contents of the private key file; the maintainer adds it in Settings → Secrets and variables → Actions, nobody else handles the key). Without the secret the release is published as a pre-release with the installers but no manifest, and the maintainer signs it on the machine that holds the key: `bun run release:publish -- --version X.Y.Z`. Building and publishing locally still works as the fallback: `bun run release:publish -- --from-dir <folder> [--from-dir <folder> ...]` (`--dry-run` first; Windows: `bun run tauri build` and `bun run package:portable`; macOS: the DMG on a Mac; Linux: built in WSL and copied to a Windows folder), with every platform's folder each time. macOS is not built in CI yet (planned for 1.2.0). The website follows by itself when `site/` or `CHANGELOG.md` changed (**Deploy site**). Pushes to `main` never release on their own.
