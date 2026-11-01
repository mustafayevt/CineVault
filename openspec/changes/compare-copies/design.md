## Context

See proposal.md for why. The behaviour is in `specs/movie-copies/spec.md`.

- Provider IDs live in `ext_id (item_id, provider, value)` with the index `ext_id_lookup (provider, value)`; the keys are `tmdb` and `imdb`. Title, year, edition, media source, availability and `trashed_entry` are columns of `item`.
- Media info is stored per video in `media_file` (`width`, `height`, `hdr_formats`, `video_bitrate_kbps`, `size_bytes`) and its audio tracks in `stream`. `cv_media::resolution_label` gives the resolution label, and `cv_app::hero_audio` picks the track the title leads with.
- `cv_db::repo::library_review::duplicate_groups` groups rows that hold the same video file, and rows of the same title and year when one of them is missing. Its comment says "two rows that both have their files are two editions, not a duplicate". Copies are exactly those rows, so the review is unchanged and its comment now names copies.
- The Library's filter row reads its counts from `lifecycle_counts` (`useLifecycleCounts`). Delete in the UI starts at `requestDelete(targets, { onDone })` in `ui/src/features/delete/deleteActions.ts`, which follows Settings › Deleted files and opens the right confirmation. The change `simplify-delete` changes that confirmation (one Delete; Yes or No for a permanent delete). This design only calls `requestDelete`, so it holds either way.

## Goals / Non-Goals

**Goals:** one cheap computation of copy groups that every view reads; a best-quality rank from media info already stored; Keep this one with no new file code; a durable Keep both; copy labels that are a pure planner pass, so renames stay previewed, journaled and undoable.

**Non-Goals:** a cached groups table, fuzzy title matching (accents, punctuation, alternative titles), new provider lookups, CLI commands.

## Decisions

### Groups are computed on read, not stored

`cv_db::repo::copies::copy_groups(conn)` runs one query over the movies that count (`kind = 'movie'`, `availability = 'online'`, `trashed_entry IS NULL`, at least one `media_file` of kind `video`), left-joined to their `tmdb` and `imdb` rows of `ext_id`, returning `id, tmdb, imdb, lower(trim(title)), year, lower(trim(edition))`. In Rust a union-find links rows that share `(edition, tmdb)`, then rows that share `(edition, imdb)`; each set remembers its TMDB ID, and a union of two sets with different TMDB IDs is refused, so an IMDb ID wrongly shared by two films never joins them. A row with neither ID is linked by `(edition, title, year > 0)` only when the rows with IDs in that bucket are already one set (or there are none, and it joins the other ID-less rows); when they are two films it stays out, because it cannot be told which one it is. Groups of two or more come back with their members in id order.

For 5,000 movies this is one indexed scan of a few thousand short rows and a linear pass, a few milliseconds, well inside the 100 ms budget of the Library queries. Because nothing is stored, Scan, Scrape, edits, Delete, Undo and Remove from library need no hook: the next read sees the change, and the UI already invalidates its queries after each of them.

Alternatives: a `copy_group` column refreshed after scans and scrapes (more write paths to keep right, stale after an edit or Undo); a pure SQL self-join (harder to make transitive across the ID and title rules).

Rows that the review already groups because they hold the same video file cannot both be online with distinct files, so the two features never claim the same pair of files.

### "Keep both" is a small table keyed by the member set

Migration (the next free number after `104`) adds:

```sql
CREATE TABLE copies_kept (
    set_id   INTEGER NOT NULL,
    item_id  INTEGER NOT NULL REFERENCES item (id) ON DELETE CASCADE,
    kept_ms  INTEGER NOT NULL,
    PRIMARY KEY (set_id, item_id)
);
```

Keep both writes one `set_id` (the next free one) with every current member, in one transaction. A group is kept when all its current members belong to one kept set; a new member makes the test fail, so the group opens again, while a member that leaves (deleted, offline, re-scraped to another film) leaves it kept. Ask again and the toast's Undo delete that `set_id`. A removed title removes its own rows by the cascade; a set left with no rows simply no longer exists. This table holds decisions, not titles, so no library row is ever deleted by it.

Alternative: a flag on `item` (cannot tell which group was kept, so a third copy would not reopen it); `review_kept` (keyed by one item and a reason, the wrong shape for a set).

### Where the groups are read

`cv_app::copies::CopiesService` (desktop and `cv-web` share it):

- `counts`: the number of movies in open groups, added to `LifecycleCounts` as `copies`, so the chip reads the same query as the Problems chip.
- `group_of(id)`: the group of one movie with its kept state, for the Movie page line and the Compare page. The Library list rows carry `copies: u8` (the group size of an open group, else 0) for the badge, filled from one `copy_groups` call per page request.
- The list filter `copies = true` restricts the Library query to the members of open groups (an `id IN (…)` of at most a few hundred ids) and orders by the group's first position in the chosen sort, then the quality rank inside the group, so copies sit together.
- `compare(id)`: one row per copy for the page, built from the stored media info, with `best` set as below.
- `keep_all(ids)` and `ask_again(id)`: write or delete the kept set; both are typed `tauri-specta` commands (and `cv-web` routes), and `ui/src/bindings.ts` is regenerated.

### Best quality from stored media info

For each copy the main video (`media_file` kind `video`, first by position; for a stacked movie the first part, with sizes summed) gives a key compared in this order:

1. resolution: the bucket of `resolution_label(width, height)` (4320p > 2160p > 1440p > 1080p > 720p > lower), so a cropped 1920×800 film is 1080p like a 1920×1080 one;
2. HDR: the best of `hdr_formats`, Dolby Vision 4 > HDR10+ 3 > HDR10 2 > HLG 1 > SDR 0;
3. audio: the track `hero_audio` picks, by `channels`, then a codec rank (TrueHD Atmos, DTS-X > TrueHD, DTS-HD MA > E-AC-3 Atmos > DTS-HD HRA > E-AC-3 > DTS > AC3 > AAC > others);
4. `video_bitrate_kbps`.

The highest key is marked when it is strictly higher than every other copy's and the copy's media info has been read (width and height known). Source and size are shown but do not rank: a bigger file is not a better one, and the source is parsed from names. The ranking is a pure function in `cv-app` with table tests.

### Keep this one is the existing Delete

The page's data can be stale (a copy deleted in another window, a drive gone offline, a copy re-scraped to another film), so the page first calls `keep_copy(keep, others)` (`CopiesService::keep_copy`, a typed command and a `cv-web` route like the others). It reads the groups again and refuses, deleting nothing, when `keep` is no longer in a group (deleted and not undone, offline, missing, re-scraped), when one of `others` is not in that same group, or when two copies of the group may be one file; otherwise it returns `others` without repeats. Only then does the page call `requestDelete(confirmed, { onDone })` with exactly those copies. Everything after that is today's Delete: the "Ask before deleting" confirmation or none, the permanent-delete confirmation when Deleted files is Delete permanently, the toast, the History entry and Undo. `onDone` runs only after a delete that went through and opens the kept copy's Movie page; a cancelled confirmation leaves the page as it was. While one Keep this one is checking, asking or deleting, every card's button is disabled, so two clicks cannot delete every copy. `keep_copy` deletes nothing itself; no new delete code exists. A kept group ("Keep both") still offers Keep this one, as the spec says, so the check takes kept groups too.

The kept copy keeps its own NFO, artwork, subtitles and folder untouched. The deleted copies' NFO and artwork go with them by the existing rules: their whole folder when they have their own, or their own files and the files named after their video in a shared folder (`Deleting never touches other titles` and `Delete takes the files named after the title` in `title-deletion`). Two copies in one folder therefore never take each other's files.

### One file through two library folders

The same folder can be in the library twice under two paths: `D:\Movies` and the network share of it, a mapped drive, a junction or a `subst` drive. Its movies then look like copies, and "Keep this one" would delete the kept copy's own files through the other path. Two guards close this:

- `compare` resolves each copy's main video and marks two copies as maybe one file when `same_file::is_same_file` says so, or, as a fallback for shares that do not report the identity of the disk behind them, when the file name (ignoring case), size and modified time (within two seconds) agree, or when either video cannot be read. `Comparison::same_file` (and `CopyRow::same_file`) carry the mark; the page then hides every "Keep this one", never says "look the same", and points at Settings › Library folders. The fallback can mark two real copies made with a copy tool that keeps the modified time; they are still deleted one by one with Delete. Being wrong that way loses nothing.
- Adding or remapping a library folder (`SourceGuard::check_with_links`) resolves the new folder and each reachable library folder of the same library: the same folder (`same_file` on the folders, or the same `canonicalize` result) is refused as `same_folder`, and a folder whose ancestor is an existing one, or that is an ancestor of one, as `inside_existing` / `contains_existing`. A library folder that cannot be reached is compared by text only, as before.

### The Compare copies page

Route `/movies/$movieId/copies`, built like the other picker pages: a header with Back (Esc), "Compare copies" and the film's title, then one card per copy in a row (wrapping at narrow widths; three or more copies scroll sideways inside the page). Each card has the poster, a "Best quality" tag when marked, the rows of the spec in a label/value grid with the winning value of each row emphasised, the folder path with Open folder, and "Keep this one" as its primary button. Under the cards: "Keep both" / "Keep all", or for a kept group the note "You chose to keep these copies" with "Ask again". Design tokens and `lucide-react` icons only; every button keyboard reachable. The visual design comes from the separate mock-up.

The "Best quality" tag only suggests: every card keeps "Keep this one". When all copies show the same Resolution, HDR, Audio and Source values and known lengths at most `SAME_LENGTH_MINUTES` (3) apart, a note above the cards says "These two copies look the same." and suggests keeping one. Two films of one title and year (a remake listed without IDs) can share everything but their length, so when known lengths differ by more than 5 minutes the page says "Lengths differ by N min — these may be different films.", marks the Length cells and never offers "Keeping one frees". The Delete and permanent-delete confirmations show each title's folder under its name (`PathText`, full path on hover), so the confirmation of Keep this one tells two copies of one title apart; this applies to every Delete, where it only adds information.

### Right-click entry

`ui/src/features/library/titleActions.ts` gets a `compare-copies` action in the title-actions group, shown only for a single movie whose list row has a group (`copies > 0`, or the kept group size from the row), labelled "Compare <n> copies" and opening `/movies/$movieId/copies`. The row therefore carries the group size for kept groups too (`copies_total`), while the badge reads only open groups. `simplify-delete` edits the Delete group of the same list; this entry sits outside it.

### Copy labels in Rename

**Where.** `cv-app/src/organize/service.rs` renders each movie's `MoviePlan`, wraps it in a `BatchItem` and hands the batch to `cv_rename::conflict::detect`. A new pass, `cv_rename::conflict::copies::label_copies(items, members, opts)`, runs on the batch before `detect`. `members` holds, for every movie of the batch that belongs to a copy group (open or kept, from `copy_groups`), its group id, its library id and its label attributes. For the members of those groups that are not in the batch, the organize service renders their plans too (never executed) and passes them as fixed, read-only participants, so a copy renamed alone still sees its twin.

**Clash.** Within one group, members whose new folder (folded by `opts.key`, so per library folder root and with the volume's case rule) or whose video destination is equal clash. Copies in different library folders do not clash and keep the plain name. Copies that already share a folder (folder renaming off, or a multi-movie folder) clash only on the video name, so only the video base name gets the label.

**Label.** Each member's attributes come from stored media info, read as the Compare page reads them: resolution bucket from `resolution_label(width, height)` mapped to 2160p, 1080p, 720p or SD; best HDR format from `hdr_formats` as DV, HDR10+, HDR10, HLG or SDR; source from `item.media_source` normalised to BluRay, Remux, WEB-DL, WEBRip, HDTV or DVD (anything else as written); the `hero_audio` track as "Atmos" for an Atmos track, else codec and layout ("TrueHD 7.1", "AAC 5.1"), with DTS-HD MA as its own name. A clashing set takes the resolution if it alone separates them; otherwise, among members of one resolution, the first attribute in the order HDR, source, audio that differs is appended ("2160p DV"). Members that still share a label are numbered by library id: when the whole set is identical the lowest id keeps no label and the others get "copy 2", "copy 3"; otherwise " copy 2" is appended to the repeated label. Labels go through `create_destination` like any rendered name.

**Apply.** The pass rewrites each labelled item's `new_folder` to `<folder> - <label>` and its base name to `<base> - <label>` with the same `change_base` that `Resolution::KeepAsVersions` uses, so the NFO, artwork named after the video, subtitles and other sidecars follow it. It never sets `shares_folder`, so two copies are never put in one folder (unlike Keep as versions, which stays a manual resolution for clashes between different movies). After the pass, `detect` runs as before: separated copies raise no `DuplicateDestination`, and any clash left (a different movie, a foreign folder on disk) is still reported.

**Nested and lone copies.** When the pass gives a folder label to a copy whose old folder holds another copy of its group (a twin kept in a subfolder), the copy is planned as `MoveFiles` instead of a folder rename, so its own files move to the labelled folder and the twin is not carried along inside it. When a copy is renamed without its twin and its new folder is the folder the twin (a fixed participant) is in now, `separate_copies` takes it out of the batch as unplanned with "Its other copy is in <folder> now. Rename both copies together."; renamed together, the twin moves out of the way in the same batch.

**Labels at apply time.** Apply plans the batch again, so the labels are computed again from the copies' media info and group as they are then. If a twin changed between the preview and Apply (its media info read again, a copy deleted or added), a label can differ from the one the preview showed. Only names change this way: the rename is still journaled, shown in History and undoable, and no file of one copy can land on the other's because `detect` runs on the plan that is executed.

**Stable.** The pass is a pure function of the templates, the group members' attributes and their ids, so the same library gives the same names and an already labelled copy plans as tidy. When a group member leaves (deleted, scraped to another film), the clash is gone and the plain name renders, which shows as a normal row the next time Rename is opened; nothing is planned or run in the background. A copy whose media info is not read yet waits like any title under "Rename waits for video details", and so do the copies it clashes with, so a label is never made from missing data.

**Drop the label after Keep this one.** `onDone` of the delete compares the kept movie's current folder and video base name with its plain rendered name (`plan_plain(id)`, the organize plan without the pass). When they differ only by a " - <label>" suffix, the toast gets a second action "Rename to drop the '- <label>' label?" that opens `/rename` for that one movie. The Rename preview, journal and Undo do the rest.

**Golden files.** For a movie in no copy group the pass changes nothing, so every existing `tests/compat/` golden file stays byte for byte. A new family `rename-copies` (`fixtures/rename/copies.json` → `expected/rename/copies.json`, a handler in `crates/cv-compat/src/handlers.rs`) pins the six spec cases: 2160p vs 1080p, DV vs HDR10, BluRay vs WEB-DL, identical copies, a single movie, and a group that lost a member.

## Safety and Undo

This change writes, moves and erases no file of its own. Keep this one hands the other copies to the existing Delete, which sends path by path to the chosen destination and records every sent path in one History entry, also when it stops part way, so Undo restores each file to its old path and never overwrites; with Delete permanently, the confirmation lists what will be erased and the entry is recorded before erasing, as today. An interrupted Keep this one leaves the kept copy untouched and the not-yet-sent files of the other copies in place; the group simply still shows on the next read. Copy labels change only the names a rename plans: they are shown in the preview first, and the executor, journal, the per-title organize mark, interruption recovery and Undo are the existing ones, so an interrupted labelled rename resumes or rolls back like any other. Labels never put two copies into one folder, so no file of one copy can overwrite the other's; any destination still taken is blocked by `detect`. The drop-the-label offer only opens the preview. Keep both and Ask again are single database transactions, undone by the toast's Undo or by Ask again. No library row is deleted automatically: deleted copies follow Delete's own row handling (hidden until Undo).

## Risks / Trade-offs

- [Two different films with the same title and year, one unscraped] → shown as copies. The page shows both posters, folders and sizes, nothing is deleted without the user's choice, Delete is undoable, and Scrape separates them by ID.
- [An edition tag on one copy only ("Theatrical" vs none)] → not copies, by the strict edition rule; the user still sees both in the Library. Preferring a missed pair over a wrong pairing keeps Keep this one safe.
- [Media info not read yet] → "Not read yet" and no mark when the top copy is unknown; Reload media info fills it.
- [The Movie page header grows by a line] → the line is one short text row; `bun run check:facts` guards the first screen.
- [A copy is re-scraped or its media info read again, and its label changes] → the next Rename offers the new label as a normal row; nothing moves on its own.
- [Rendering the plans of group members outside the batch costs time] → only for movies in a copy group, a few per batch; their plans are not executed or shown.
- [Chip count on every counts read] → one extra query of a few milliseconds per counts request, which is cached for 30 seconds in the UI.

## Migration Plan

One additive migration creating `copies_kept`; older data needs nothing (no group is kept). Rolling back the app leaves an unused table.
