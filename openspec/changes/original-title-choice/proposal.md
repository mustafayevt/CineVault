## Why

A library that mixes languages wants some titles under their original title and others under the title in the metadata language (#96). Today the only way is to type the original title into the Title field of each movie by hand, which loses the localized title and takes hours on a large library. The `${originalTitle}` naming token exists, but Settings › Naming offers no button for it, so people do not find it, and it only changes the file names, not the library or the NFO.

## What Changes

- **A choice of title per movie and TV show: Localized or Original.** With Original, the original title is the title CineVault uses everywhere: the library grid and list, the title page, the search, `${title}` in the naming templates, the NFO `<title>` and the sort order. `<originaltitle>` stays the original title. The localized title is kept, so switching back restores it exactly; neither value is ever lost. A title whose original title is empty or the same as its localized title offers no choice.
- **Edit details** shows "Title to use: Localized | Original" under the Title and Original title fields when there is a choice; the Title field is then labelled "Localized title". A line says which title the lists, the file and folder names and the NFO use. Saving and Undo work like every other field.
- **Selection bar and right-click menu:** "Use original title" and "Use localized title" for the selected movies or shows, under More. The NFO files are rewritten, titles whose names would change show Rename pending (nothing is renamed on its own), and a toast confirms with Undo, which switches back exactly the titles that changed.
- **Scrape honours the choice.** A re-Scrape of a title set to Original keeps the original title as its title and refreshes the kept localized title; a new original title from the provider becomes the title in use.
- **A rule for new titles:** Settings › Providers, beside the metadata language, gets "Keep the original title" with a list of original languages (empty by default). A movie or show scraped for the first time whose original language is in the list starts as Original. "Use for titles I have" applies the rule once to the titles already in the library, with Undo. The original language is already stored for movies and shows (TMDB and TheTVDB give it); nothing new is fetched.
- **Settings › Naming** gets an {OriginalTitle} token button for the movie templates and {ShowOriginalTitle} for the TV episode template, with a line that explains {Title} follows each title's choice and {OriginalTitle} never does, with an example.
- **NFO:** `<title>` is the title in use and `<originaltitle>` the original title. While the original title is used, the NFO also keeps the localized title in CineVault's own `<localized_title>` element, so a library built again from its files gets the choice back; the element is left out otherwise.
- **Storage:** migration 109 adds `use_original_title` and `localized_title` to the item table; every existing title stays as it is (Localized). The kept localized title is searchable.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `metadata-editing`: a new requirement for the choice of title and its place on Edit details.
- `library-browsing`: a new requirement for "Use original title" and "Use localized title" on selected titles.
- `metadata-providers`: a new requirement for how Scrape treats the choice and for the original-language rule.
- `nfo-files`: a new requirement that the NFO title follows the choice and that reading an NFO back keeps it.
- `renamer`: *Naming templates in Settings* lists the original title tokens and the explaining line.

## Impact

- Rust: `cv-domain` (the choice on `ItemCore`, the scrape write path), `cv-db` (migration 109, item columns, search keys), `cv-nfo` (`localized_title` in the movie and show models, writers and readers), `cv-app` (Edit details field `title_choice`, `EditService::set_title_choice`, `titles_in_languages`, the scrape rule, `ScrapeSettings::original_title_languages`, the NFO model and read-back), `cv-template` (test only), `cv-ipc`, `cv-web` and `src-tauri` (two commands: `set_title_choice`, `use_original_title_for_languages`).
- UI: Edit details, the title actions, Settings › Providers and › Naming, the English messages, `bindings.ts`.
- NFO and rename output for titles that keep their localized title is unchanged (the golden files stay as they are).

## Out of scope

- Episodes and collections have no choice; an episode follows its show through `${showTitle}`.
- No library filter for the original language; "Use for titles I have" covers the bulk case.
- The CLI has no edit command, so it gets none here (its Scrape follows the rule through the shared settings).
- The display-only setting "Prefer the original title" stays as it is.
