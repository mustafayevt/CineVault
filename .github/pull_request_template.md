## What and why

<!-- What does this change do, and why? Link the issue (Fixes #123) and, for a feature or a behaviour change, the OpenSpec change folder. -->

## How it was checked

<!-- Tests added or updated, manual steps, screenshots for UI changes (both themes). -->

## Checklist

- [ ] Commits follow Conventional Commits (`feat(scan): …`, `fix(nfo): …`)
- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` and `cargo test --workspace` pass
- [ ] In `ui/`: `bun run lint`, `bun run typecheck`, `bun run format:check` and `bun run test` pass
- [ ] Behaviour changes are reflected in `openspec/` (a change folder, or the spec for a small fix)
- [ ] `ui/src/bindings.ts` is regenerated when IPC types changed
- [ ] Golden files in `tests/compat/` are updated only for intended output changes, and the diff was reviewed
- [ ] No API keys, personal paths or other apps' names in code, tests or text
