# Spec Delta

## ADDED Requirements

### Requirement: Server checks
The Rust workspace checks SHALL cover the server crates as they cover the rest: `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace`. A parity test SHALL compare the desktop registry with the server dispatcher, matching command names and argument names, and SHALL fail when a command is missing from either side or an argument differs. HTTP tests SHALL cover sign-in, sessions, CSRF, the trusted proxy, image headers, media range requests, the folder browser's root jail, upload atomicity across volumes, the owner file and the secrets backend. A DOM test SHALL render the application shell in browser mode and fail when the shim's `metadata`, `transformCallback`, event or `convertFileSrc` contract changes. A Linux CI job SHALL build the container image, start it against a fixture data folder and run a scan, a rename and an artwork write before checking the health endpoint.

#### Scenario: A command only in the desktop app
- **WHEN** a contributor adds a command to the desktop app and not to the server
- **THEN** `cargo test --workspace` fails and names the missing command

#### Scenario: An argument renamed
- **WHEN** the server dispatcher expects `stop_jobs` and the desktop registry sends `stopJobs`
- **THEN** the parity test fails and names the command and both names

#### Scenario: The shim contract changes
- **WHEN** a Tauri update removes `metadata.currentWindow`
- **THEN** the shell render test fails before a browser sees a blank page

#### Scenario: Container smoke test
- **WHEN** CI builds the image and starts it on a fixture data folder
- **THEN** the scan, rename and artwork write succeed, `/healthz` answers 200, and the test fails otherwise

### Requirement: Server scripts
The repository SHALL offer `bun run server:dev` (build the UI and start the server against the developer's data folder), `bun run server:build` (the release server program), `bun run server:image` (build the container image), `bun run server:package` (the `.deb` and `.rpm`) and `bun run server:tarball`. `AGENTS.md` SHALL list them with the other commands, and `scripts/check-scripts.mjs` SHALL fail when a `bun run` command named in `AGENTS.md` or `README.md` does not exist in a `package.json`.

#### Scenario: Local server
- **WHEN** a contributor runs `bun run server:dev`
- **THEN** the interface is built and the server starts on the loopback address with the local data folder

#### Scenario: A command named in the docs does not exist
- **WHEN** a script is renamed and `AGENTS.md` still names the old one
- **THEN** `bun run check:scripts` fails and names the missing command

#### Scenario: Release artifacts
- **WHEN** a maintainer runs `bun run server:image`, `server:package` and `server:tarball`
- **THEN** the image, both packages and the tarball are produced and each reports the workspace version

### Requirement: Notices and version
The third-party notices SHALL be regenerated with `bun run notices` when the server adds dependencies (`axum`, `tower-http`, `argon2`, `chacha20poly1305` and the stream implementation), and the version check SHALL cover the image tag, the server package versions and the tarball name as well as the four project files.

#### Scenario: A new dependency without notices
- **WHEN** a server dependency is added and `bun run notices` is not run
- **THEN** the notices check fails and names the licence entry that is missing

#### Scenario: A version mismatch
- **WHEN** the image tag or a package version differs from the workspace version
- **THEN** `node scripts/check-version.mjs` fails and names the file or tag
