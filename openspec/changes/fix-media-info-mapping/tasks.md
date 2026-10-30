## 1. Mapping

- [x] 1.1 Main video skips the Blu-ray enhancement layer; a Dolby Vision layer from another video stream (MediaInfo's own or the 0x1015 enhancement layer) is merged into the main picture's HDR formats and details. Verified by `cargo test -p cv-media --lib mediainfo` and the recorded Ultra HD Blu-ray field set in `cargo test -p cv-media --test mediainfo_mapping`.
- [x] 1.2 Dolby Vision profile from every codec string (`dvhe`, `dvh1`, `dvav`, `dva1`, `dav1`) and from the combined `HDR_Format/String`. Verified by the mapping unit tests.
- [x] 1.3 E-AC-3 named `AC-3` in Blu-ray stream files, channels without height speakers, frame rate of field-stored interlaced video. Verified by the mapping unit tests with recorded field values.

## 2. Blu-ray clip information

- [x] 2.1 Parse `CLIPINF/*.clpi` (languages, dynamic range, HDR10+ flag), find the main clip's file or its `BACKUP` copy, and complete the snapshot in the disc analysis. Verified by `cargo test -p cv-media --test disc_clip_info` over a folder and an ISO image.

## 3. Library

- [x] 3.1 Migration 103 marks the affected videos as not read. Verified by `cargo test -p cv-db --lib migration_103`.

## 4. Integration

- [x] 4.1 `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`, the changelog and `bun run site:changelog -- --check`. Human check (done read-only on a real library): an Ultra HD Blu-ray folder shows Dolby Vision profile 7 over HDR10 and its track languages.
