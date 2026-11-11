## Context

`ItemCore` has `title`, `original_title` and `original_language` (TMDB and TheTVDB fill the language; it is stored for movies and shows since 1.x). Every reader (lists, title pages, search, naming tokens, NFO writer, sort) reads `title`. The scrape write path (`cv_domain::apply_scrape`) copies the provider's `title` into `title`. Edit details works on field keys (`title`, `original_title`, ...) through `cv_app::edit`, and its Undo saves the previous values back.

## Goals / Non-Goals

**Goals:** one choice per movie and show that every reader follows without change; never lose the localized or the original title; Scrape keeps the choice; a simple rule for new titles; bulk switching with Undo.

**Non-Goals:** a choice for episodes or collections, an original-language filter, a CLI edit command.

## Decisions

### `title` stays the title in use; the localized title waits aside

Two columns on `item` (migration 109): `use_original_title` (0/1) and `localized_title` (empty unless the original title is used). With the choice on, `title` holds the original title and `localized_title` the title in the metadata language. Every reader keeps reading `title`, so the grid, the list, the title page, the search, `${title}`, the NFO `<title>`, the sort and `${showTitle}` of the episodes follow with no change of their own.

Alternatives: computing the effective title in every reader (many readers, easy to miss one) or keeping the localized title always in its own column (every writer of `title` would have to keep both in step). The chosen form touches only the writers that know about the choice.

`ItemCore` keeps the invariant through a few methods: `localized_title()`, `set_localized_title`, `set_original_title` (the title follows the original title while it is used; an emptied original title switches back to the localized title, so the title is never emptied), `choose_original_title(bool)` (refuses when there is no different original title, keeps the other title) and `settle_title_choice` for writers that set the title as a whole. Only movies and shows store a choice (`Item::has_title_choice`); the item repository writes 0 and '' for every other kind.

### Scrape writes the localized title

The scrape field Title is the provider's title in the metadata language: `slot` and `copy_field` read and write `localized_title()`, and Original title writes through `set_original_title`. So a re-Scrape of a title set to Original refreshes the kept localized title and keeps (or updates) the original title in use, and the scrape preview compares like with like. Field locks keep working on the same field: a locked Title keeps the localized title; the choice does not depend on it.

### The rule applies at the first Scrape

`metadata.original-title-languages` (comma-separated two-letter codes, empty by default) is read into `ScrapeSettings::original_title_languages`. In `apply_scraped_in`, in the same transaction as the scrape write, a movie or show with no scraper recorded yet (`last_scraper_id` empty, so never scraped by CineVault) whose original language is listed switches to Original, unless its Title field is locked. A title scraped before keeps whatever it has, so a re-Scrape never undoes the owner's choice and changing the list never flips titles behind the owner's back. "Use for titles I have" (`use_original_title_for_languages`) applies the list once to the titles already in the library, through the bulk path below, with Undo.

### Edit details: one more field, `title_choice`

`title_choice` (`localized` or `original`) is a field of movies and shows with no lock key. The `title` field reads and writes the localized title. A save applies `title_choice` last, so a choice saved together with new titles uses them whatever order the page sends; Undo is the usual save of the previous values. Choosing Original without a different original title is refused with a sentence on the field.

### Bulk: `EditService::set_title_choice`

One transaction over the ids: each movie or show whose choice changes is stored; titles without another original title, other kinds and titles no longer in the library are counted as unchanged. Then the NFO files of the changed titles are rewritten with the NFO settings (the same `NfoService::rewrite` the Edit page uses), and the command announces `Details` and `Stage` and calls `OrganizeStateService::note_changed`, as a save on Edit details does, so Rename pending shows where the names now differ. Nothing is renamed. Locks do not stop it: like Edit details it is the owner's own edit. The reply lists the changed ids; Undo calls the command again with the other choice on exactly those ids, which restores them because neither title was lost.

### NFO

`<title>` is `title` and `<originaltitle>` is `original_title`, as before. The NFO is the library's complete copy, so while the original title is used the kept localized title goes into CineVault's own elements as `<localized_title>`, after `original_language` (movies and shows, `MovieNfo::localized_title`, `ShowNfo::localized_title`); it is left out otherwise, like every empty addition. The golden files are written without CineVault's additions and do not change; the NFO of a title that keeps its localized title is byte for byte what it was. Reading an NFO back (`merge_common`) sets the title as the file says, then `adopt_title_choice`: a `<localized_title>` beside a title that equals the original title turns the choice on (so a library built again from its files gets the choice back), and `settle_title_choice` ends it when the title read is not the original title (a file edited elsewhere). The element is in the known-element lists, so it is never kept as unknown content.

### Search

The FTS title column and the folded `search_alt` key also hold `localized_title`, so a title that uses its original title is still found by its localized one. Existing rows need no rewrite: `localized_title` is empty for all of them.

## Risks / Trade-offs

- A failed NFO write after a switch leaves an NFO with the old title; a later scan that reads it back would take that title as the title in use (as for any edit whose NFO write failed). The toast warns and Rewrite NFO fixes it.
- The rule depends on `last_scraper_id`: a title imported from an NFO written by an earlier CineVault counts as scraped and keeps its title; "Use for titles I have" covers it.

## Data safety

No file in a media folder is renamed, moved or deleted by this change. The only file writes are the NFO rewrites, which go through `cv-fs` atomic replace as every NFO write does, so an interruption leaves the old or the new NFO, never a partial one. The library change is one transaction per command; Undo is the same command with the other choice, and Edit details' Undo is the usual save of the previous values. Names change only through Rename, with its preview, journal and Undo.

## Migration

Migration 109 adds the two columns with defaults (0, ''), so every title a library holds stays exactly as it was; the pre-migration backup is taken as for every migration. A newer database is refused by older builds as usual.
