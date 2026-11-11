## 1. Backend: scan one place, progress in titles

- [x] 1.1 `cv-jobs`: `UnitProgress`, `ItemControl::set_units`, `JobSnapshot.units`, `ItemWork::prepare`. Verify: `cargo test -p cv-jobs --test units`.
- [x] 1.2 `cv-db`: `plan_availability_of` probes only the wanted titles. Verify: `cargo test -p cv-db --test availability` (`a_scoped_plan_probes_and_changes_only_the_wanted_titles`).
- [x] 1.3 `cv-scan`: the walk's focus and observer, `count_titles`, `rescan_folder`, scoped availability, `RescanTarget::Folder`, the rescan job's `prepare` and progress (`rescan::progress`). Verify: `cargo test -p cv-scan` (`rescan_folder.rs`: other folders untouched, missing and back, new files, a title in a group folder, one show of several, the count agrees with the walk; `progress` unit tests: phases, throttling, the total never drops, the estimate).
- [x] 1.4 `cv-app`: `ScanService::scan_folder` and `ScanService::rescan_title`, named jobs, `ScanError::NoFolder`. Verify: `cargo test -p cv-app --test scan_one_place`.
- [x] 1.5 `cv-app`: `OrganizeService::apply_with` reports progress after each title. Verify: `cargo test -p cv-app --lib a_rename_tells_how_far_it_got_after_each_title`.
- [x] 1.6 IPC: `rescan_folder`, `rescan_title`, `JobProgress.units`, the progress channel of `organize_apply`, the browser server's dispatch and command list; bindings regenerated. Verify: `cargo test -p cv-ipc -p cv-web`, `CINEVAULT_UPDATE_BINDINGS=1 cargo test -p cinevault-app generated_bindings`.

## 2. UI: scan one place and progress that moves

- [x] 2.1 Scan backend and actions: `scanFolder`, `scanTitle`, toasts "Scanning <name>", errors to Activity. Verify: `scan.test.tsx`.
- [x] 2.2 Drives card "Scan this folder" per library folder; Settings › Library folders "Scan this folder"; title pages "Scan again" in the ⋯ menu and in the files notice. Verify: `drivesPage.test.tsx`, `moviePage.test.tsx` (menu order), `filesNotice.test.tsx`, `settingsSections.test.tsx`.
- [x] 2.3 Progress lines in titles (counting, working, time left) in the sidebar card and its list. Verify: `scanProgress.test.tsx`, `sidebar.test.tsx`.
- [x] 2.4 Activity "Running now". Verify: `scanProgress.test.tsx` (Running now).

## 3. UI: rename feedback

- [x] 3.1 `runRename` outside the page, the running rename on the progress card, the result toasts, failures in Needs attention with "Open in Rename". Verify: `renamePage.test.tsx`, `runRename.test.ts`, `needsAttention.test.tsx`.

## 4. Integration

- [x] 4.1 `cargo fmt`, `clippy`, the crate tests; `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `bun run shots` for `/drives`, a movie page and the sidebar during a synthetic scan; `openspec validate scan-and-rename-feedback --strict`; the changelog lines and `bun run site:changelog`. Verify: all pass.
