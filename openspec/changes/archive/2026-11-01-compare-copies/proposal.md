## Why

People who download the same movie twice, for example a 1080p WEB-DL and a 2160p BluRay in two folders, end up with two titles that both have their files. The library lists them as two unrelated movies: nothing says they are the same film, which one is better, or offers to get rid of the worse one. The missing and duplicate review only looks at rows that lost their files, so these copies are never pointed out.

## What Changes

- **Copies of a movie.** Two or more movies whose files are all present are copies when they share a TMDB or IMDb ID or, when one of them has neither ID, the same title and year. Movies with different editions (Extended and Theatrical) are not copies. TV shows are left out (duplicate episodes already have their own filter).
- **"Copies <n>" chip** in the Library's filter row, after "Problems", shown for movies while at least one movie has an open copy. It lists only those movies, the copies of each film next to each other.
- **"2 copies" badge** on the poster card of each movie in an open group, and a line "You have 2 copies · Compare" in the Movie page header of every movie that has copies.
- **Compare copies page** (`/movies/<id>/copies`): the copies side by side with resolution, HDR, audio, source, size, video bitrate and folder; the best one marked "Best quality".
- **"Keep this one"** on each copy deletes the other copies with the existing Delete, so it follows Settings › Deleted files exactly as Delete does (to the Recycle Bin or a trash folder with Undo, or the permanent-delete confirmation when the setting is Delete permanently).
- **"Best quality" is a suggestion**: every card offers "Keep this one". Identical copies are called out ("These two copies look the same.").
- **"Compare 2 copies"** in the title right-click menu of a movie with copies, with the title actions.
- **Copy labels in Rename.** Each copy keeps its own folder. Only when copies would end at the same folder or video path, Rename adds " - <label>" to the folder and video names (and the files named after the video): the resolution, then the first of HDR, source and audio that differs ("2160p DV", "2160p WEB-DL"), or "copy 2" for identical copies. Labels are stable; when the clash is gone, the next Rename offers the plain name as a normal preview row, and the Undo toast of Keep this one offers "Rename to drop the '- <label>' label?". Everything goes through the preview, journal and Undo.
- **"Keep both"** (or "Keep all" for three or more) remembers the decision in the library, so the group stops counting and loses its badges; a new copy found later opens the group again. The page of a kept group offers "Ask again".

## Capabilities

### New Capabilities

- `movie-copies`: what counts as copies of a movie, the Copies chip and filter, the poster badge, the Movie page line, the right-click entry, the Compare copies page with its best-quality mark, Keep this one, Keep both, and the copy labels Rename adds when copies' names clash.

### Modified Capabilities

None. The chip, the badge, the Movie page line, the menu entry and the copy labels add to the Library's filter row, the poster card, the title header, the right-click menu and the renamer's conflicts without changing what those requirements already say, so they are added requirements of `movie-copies`; touching the existing blocks would push them over the 500-character limit. Deletion itself is unchanged (`title-deletion`), and the renamer's "Inline rename conflicts" keeps applying to clashes between different movies.

## Out of scope

- Merging copies into one title with several versions, or moving their files into one folder.
- TV shows and episodes (duplicate episodes stay with `tv-shows`).
- Deleting a copy automatically or on a schedule; every delete is the user's choice on the Compare copies page or in the Library.
- Carrying the watched state, play count, tags, note or artwork of a deleted copy over to the kept one.
- Copies whose files are missing or offline: the missing and duplicate review keeps handling those.
- The CLI.

## Impact

- `crates/cv-db`: a copies query over `item`, `ext_id` and the video `media_file` rows; a `copies_kept` table in a new migration; the Library list query takes a copies filter and its order.
- `crates/cv-app`: a copies service (groups, counts, the compare rows with the best-quality rank, keep both and ask again); the lifecycle counts gain the copies count; `library_review.rs` keeps its rule and its comment is updated to point at copies.
- `crates/cv-rename`: a copy-label pass in `conflict/` that runs before conflict detection; `crates/cv-app/src/organize/` gives it the copy groups and each copy's label attributes. `tests/compat/`: a new `rename-copies` fixture; existing golden files do not change.
- `src-tauri` and `cv-web`: typed commands for the copies list, the compare rows, keep both and ask again; `ui/src/bindings.ts` regenerated.
- `ui/`: the Copies chip, the poster badge, the Movie page line, the right-click entry, the Compare copies page and route, the drop-the-label toast action, and English messages. Keep this one reuses `ui/src/features/delete/`; the label rows reuse the Rename preview.
- Docs: `CHANGELOG.md`, `site/changelog.html`, the README, the landing page and the screenshots (`.github/assets/`, `site/img/`).
