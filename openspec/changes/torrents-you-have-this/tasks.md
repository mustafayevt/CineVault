## 1. Backend

- [x] 1.1 `cv-app` torrents library: every library movie with the TMDB ID as a copy (codec, hero audio, edition, languages, size, file name, item id), best first in the `upgrade` order; the compared copy is the best one; a show or season search compares with the quality most held episodes in scope share; a show's holding carries its missing episodes. Verify: unit tests for the copy order, the shared show quality and the missing episodes.
- [x] 1.2 `cv-ipc`: `libraryCopies` (replacing `libraryCopy`) and `missingEpisodes` on the title details, `episodes` on each release; regenerate `ui/src/bindings.ts`. Verify: `cargo test -p cv-torrent -p cv-app -p cv-ipc -p cinevault-app`, clippy, fmt.

## 2. UI

- [x] 2.1 Comparison model: compare rows, the reason and the verdict sentence from a release and a copy; show episode tags (Fills a gap / In your library / Better) from the span and the held and missing episodes. Verify: unit tests.
- [x] 2.2 "You have this" card replacing the Yours line, with Open in library, Look for a better copy / Find missing episodes and Search anyway; several copies listed. Verify: page tests.
- [x] 2.3 The tag as a button opening "Compare with yours", its tooltip saying why; the compare block in the Latest releases panel. Verify: tests.
- [x] 2.4 "Only better than mine" and "Fills a gap" chips, `?only=` in the address, the chip's empty state. Verify: tests.
- [x] 2.5 Find a better copy (Movie page ⋯) and Find missing episodes (Show page ⋯). Verify: title page tests.

## 3. Integration

- [x] 3.1 Rust checks; `bun run lint`, `typecheck`, `test`, `format:check` in `ui/`; `openspec validate torrents-you-have-this --strict`; `bun run shots -- --routes /torrents` in both themes; changelog line and `bun run site:changelog`.
