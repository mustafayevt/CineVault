# Contributing to CineVault

Thanks for your interest in CineVault! Bug reports, ideas, documentation fixes and code are all welcome. This page explains how to set up the project, which checks to run and how changes are proposed. [AGENTS.md](AGENTS.md) has the full list of commands and conventions (it is written for people and AI coding assistants alike).

By taking part you agree to follow the [code of conduct](CODE_OF_CONDUCT.md).

## Reporting bugs and asking for features

- Search the [existing issues](https://github.com/mustafayevt/CineVault/issues) first.
- Questions about setting up, naming templates, which format to write or how a feature is meant to behave go to [Discussions](https://github.com/mustafayevt/CineVault/discussions). Issues are for bugs and concrete feature requests.
- Use the issue templates: a bug report needs the CineVault version, your operating system, the steps to reproduce and, if you can, a log.
- Never paste API keys into an issue. Security problems are reported privately (see [SECURITY.md](SECURITY.md)).

## Setting up

You need:

- the [Rust](https://rustup.rs) stable toolchain (the MSVC target on Windows);
- [bun](https://bun.sh);
- the [Tauri 2 prerequisites](https://v2.tauri.app/start/prerequisites/) (WebView2 on Windows, the WebKitGTK and `libdbus` development packages on Linux, the Xcode command-line tools on macOS);
- the [OpenSpec CLI](https://github.com/Fission-AI/OpenSpec) if you work on a feature.

Then, from the repository root:

```sh
bun install
cd ui && bun install && cd ..
bun run tools:fetch      # pinned MediaInfo, FFmpeg and helper tools into .dev-tools/
bun run tauri dev        # start the desktop app
```

The UI also runs on its own in a browser (`bun run dev` in `ui/`) against a built-in synthetic library, which is the quickest way to work on screens.

To Scrape in a dev build you need your own free [TMDB API key](https://www.themoviedb.org/settings/api), entered in the app; it is stored in the OS keyring. The repository and the build never contain API keys.

## Checks to run before a pull request

Rust (on Windows Git Bash, first `export PATH="$HOME/.cargo/bin:$PATH"`):

```sh
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

UI (inside `ui/`):

```sh
bun run lint
bun run typecheck
bun run format:check
bun run test
bun run build
```

CI runs the same checks on every pull request, on Windows and Linux (macOS comes with the 1.2.0 work), and the two build jobs must pass before a pull request is merged. Pull requests are squash-merged with a Conventional Commit title.

Also, when it applies:

- **IPC types changed:** regenerate `ui/src/bindings.ts` with `CINEVAULT_UPDATE_BINDINGS=1 cargo test -p cinevault-app` (or by starting the debug app) and commit it.
- **NFO, rename or name-parsing output changed on purpose:** update the golden files with `CINEVAULT_UPDATE_COMPAT=1 cargo test -p cv-compat` and review the diff line by line; see [tests/compat/README.md](tests/compat/README.md).
- **UI changes:** check the screens in both themes with `bun run shots -- --routes <routes>` and attach screenshots to the pull request; run `bun run check:facts` after changing the Movie page header, facts or Technical card.

Tests must not create large files or touch real media folders; use temporary directories. Tests that call live services are opt-in through an environment variable and skip without it.

## How features are proposed: OpenSpec

The behaviour of every capability is written down in [`openspec/specs/`](openspec/specs). A new feature or a change of behaviour goes through an OpenSpec change before code:

1. **Discuss** the idea in an issue first, so we agree it fits CineVault.
2. **Propose** it: create `openspec/changes/<name>/` with `proposal.md` (why and what), `design.md` (how, for anything non-trivial), delta specs under `specs/<capability>/spec.md` and `tasks.md`. `openspec validate <name> --strict` must pass. The `/opsx:propose` command in `.claude/commands/` does this with an AI assistant.
3. **Apply** it: implement the tasks, ticking them off in `tasks.md`, with tests.
4. **Archive** it after the pull request is merged (`/opsx:archive` or `openspec archive <name>`), which folds the delta specs into `openspec/specs/`.

Bug fixes that restore specified behaviour, refactors, tests and documentation fixes need no change folder.

## Commits

Commit messages follow [Conventional Commits v1.0.0](https://www.conventionalcommits.org/en/v1.0.0/):

```
<type>(<optional scope>): <description>

<body: why and what>
```

Types: `feat`, `fix`, `refactor`, `test`, `docs`, `chore`, `build`, `ci`, `perf`, `style`. Mark breaking changes with `!` or a `BREAKING CHANGE:` footer. Keep each commit to one logical change; the scope is usually the crate or area (`scan`, `nfo`, `rename`, `ui`, `library`…).

## Pull requests

- Branch from `main` and keep the pull request focused on one topic.
- Fill in the template: what and why, how it was checked, the linked issue and OpenSpec change.
- Follow the conventions in [AGENTS.md](AGENTS.md): design tokens only in the UI, keyboard reachable controls, one verb per job (Scan, Scrape, Rename), data safety first (nothing is deleted without the user's choice, renames are undoable), and compatibility described as the Kodi, Jellyfin, Emby or MediaPortal formats without naming other media-manager apps.
- Use only code you have the right to contribute. Contributions are licensed under the [Apache License 2.0](LICENSE), like the rest of the project.
- Expect review comments; small follow-up commits are fine and are squashed or kept as the maintainer sees fit.
