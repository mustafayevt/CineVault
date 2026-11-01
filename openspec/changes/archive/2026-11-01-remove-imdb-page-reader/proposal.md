## Why

CineVault reads IMDb by loading its web pages with a browser's User-Agent and taking the data embedded in them. IMDb offers no public API for this, its conditions of use forbid automated access, and it answers often with a human check, so the source shows "IMDb is not available right now." (issue #26). IMDb's data is already available through official routes: TMDB gives every title's IMDb ID, OMDb (the user's own key) answers Scrape searches with IMDb's details and ratings, and IMDb's free ratings dataset gives IMDb ratings. CineVault should use only those.

## What Changes

- **BREAKING** The IMDb web-page provider is removed: it no longer answers Scrape searches, offers posters and fanart, or reads the Top 250 charts. No request goes to imdb.com.
- IMDb data comes through OMDb: with an OMDb key saved, Scrape searches ask OMDb as they do today. The OMDb key's hint in Settings › Providers and in onboarding says so: "Adds IMDb data to Scrape, and IMDb, Rotten Tomatoes and Metacritic ratings."
- The IMDb Top 250 rank stays a field of every title (from an NFO or Edit details, as a filter, a renamer token and in the NFO), but CineVault no longer fetches the chart. The unused `fetch_imdb_top250` command goes.
- Titles scraped from IMDb before keep their details, artwork and IDs. IMDb IDs, the IMDb ratings dataset, OMDb ratings and links to IMDb pages are unchanged.

Out of scope: OMDb's own behaviour and limits, the other web-page providers (OFDb, fernsehserien.de), and any paid IMDb API.

## Capabilities

### New Capabilities

None.

### Modified Capabilities

- `metadata-providers`: "Keys the user can enter" (the OMDb hint), "Providers that take part in a scrape" (IMDb leaves the list); "A human check is unavailable, not a failure" is replaced by "A web site's human check is unavailable, not a failure", which no longer names IMDb.
- `artwork`: "Artwork providers" (IMDb leaves the order).
- `ratings`: "IMDb Top 250 rank" (no chart fetch).
- `onboarding`: "Ratings keys step" (the OMDb hint).

## Impact

- `crates/cv-providers`: remove `imdb/` (provider, page reader, images, Top 250 source), its fixtures and tests (`tests/imdb.rs`, `tests/imdb_live.rs`, `tests/fixtures/imdb/`), and the `Top250Source` trait if nothing else implements it.
- `crates/cv-app`: provider registration and artwork order (`provider_setup.rs`, `artwork::default_provider_order`), `top250.rs` and the IMDb cases in `tests/unavailable_providers.rs`; `crates/cv-ipc/src/ratings.rs` and `src-tauri` lose `fetch_imdb_top250`; `ui/src/bindings.ts` regenerated.
- `ui`: the OMDb hint in Settings › Providers and onboarding, and any IMDb entry in synthetic data.
- README, landing page and `CHANGELOG.md` where IMDb is described as a source.
