## Why

Movies scraped by CineVault get no tags. Titles whose NFOs came from before CineVault carry the TMDB keywords as `<tag>` lines, but a CineVault scrape never imports keywords, so every newly added movie has an empty tag list and the library is split in two (#83).

## What Changes

- **Tags are a scrape field.** "Tags" joins the movie, TV show and episode scrape field selections, selected by default, and a scrape writes the provider's keywords as tags.
- **Added, never replaced.** The keywords are added to the tags a title already has (from its NFO), so a tag of the user's own stays. A change of match keeps the tags, and "write only empty fields" leaves filled tags alone.
- A field list chosen earlier in Settings keeps what it names; Tags is on for everyone who never changed it.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `metadata-providers`: *What a scrape writes* includes tags; *Provider keywords are never tags* is replaced by *Provider keywords become tags*.

## Impact

- `cv-domain` (`MetaField::is_scrapable` removed, tags merged on apply), `cv-app` scrape settings, `cv-providers` universal field list, the Settings field checklists and their label.
- No schema, IPC or NFO format change: the NFO already writes every tag.
