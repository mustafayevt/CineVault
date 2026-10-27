# Provider fixtures

Hand-built responses in each provider's documented JSON shape, replayed by the local mock server
in `tests/support`. Only the fields the providers read are present. Titles, names and image hosts
(`img.example.test`) are made up or generic; nothing is copied from a service. The opt-in live
tests (`*_live.rs`) confirm the shapes against the real services when a key is at hand.

- `omdb/`: `title_*.json` (a movie with the Rotten Tomatoes fields, a series, an episode, a title
  with only `N/A` values), `search_*.json`, `season_*.json`, and the `"Response":"False"` error
  answers (invalid key, daily limit, not found, too many results).
- `fanart/`: `movie_example.json` and `show_example.json` in Fanart.tv's v3 shape (a list of
  `{id, url, lang, likes}` per type name such as `movieposter`, `hdmovielogo`, `showbackground`;
  season entries add `season`; `likes` is text; `lang` `00` or empty means no text), the
  `{"status":"error","error message":...}` bodies and `latest.json` (the credential check). Names
  and hosts are made up.
- `trakt/`: `movie_*.json` and `show_*.json` (summary, people, translations, seasons with
  episodes), `search_movie.json`, `id_lookup_*.json`, and the 401/403 error body.
- `tvmaze/`: `show_plain.json` and `show_embedded.json` (with `embed[]` cast, crew, images and
  seasons), `search_shows.json`, `episodes_aired.json`, `alternatelists*.json` and
  `alternateepisodes_*.json`.
- `moviemeter/`: `film_*.json`, `search_*.json` and the key error body. MovieMeter publishes no
  schema we could check offline, so the film fields follow the ones the provider reads
  (`id`, `title`, `alternative_title(s)`, `year`, `imdb`, `duration`, `average` on five stars,
  `votes_count`, `genres`, `countries`, `directors`, `actors`, `plot`, `posters`).
- `ofdb/`: `search_results.html` (the result table of OFDb's search page: film rows with
  `[OT: ...]` and `[AT: ...]` notes, a series row, a person row and a row with no year),
  `search_empty.html`, `film_page.html` (title heading, microdata `description`, `genre`,
  `countryOfOrigin`, `director`, `actor`, rating, the detail list, the IMDb link, poster and
  backdrop), `plot_page.html` (the full plot), `film_short.html`, and `*_changed.html` pages
  whose structure differs ("page format changed"). The markup was modelled on OFDb's real pages
  (checked against them by hand); the film, people and texts are made up.
- `fernsehserien/`: schema.org JSON-LD and microdata pages for a show, a TV film, a search, an
  episode guide (with season pages) and a cast page. fernsehserien.de answers programs with a
  bot-check challenge, so these are hand-built from the structured data the site is known to
  embed, not recorded; the opt-in live test confirms them.
- `thesportsdb/`: `all_leagues.json`, `league_*.json`, `seasons_*.json`, `events_*.json` in the
  shape of the v1 API (a league is a show, an event an episode), plus the plain-text error the
  service sends for a disabled key. League and team names are made up.
- `mpdb/`: movie and search JSON, and the 401 body (the only response seen on the real service).
  MPDb.tv publishes no schema; the movie fields are the ones the lenient reader looks for.
- `anidb/`: `anime_*.xml` (a series with creators, tags, resources, characters and episodes, and
  a film), `error_*.xml` (not found, banned, unknown client, other), a truncated document, the
  title dump (`anime-titles.dat` and the gzipped `.dat.gz`) and a blocked-page body.
- `anime_offline_db/`: a few lines in the anime-offline-database format (`sample.jsonl`,
  `sample_next.jsonl` for a later release). The real file is about 60 MB and is never committed.

- `kodi/`: sample Kodi add-ons for the XML scraper interpreter. `addons/` holds two small scrapers
  written for these tests (`metadata.sample.films`, `metadata.sample.series`; their site address is
  the add-on setting `baseurl`, which the tests point at the mock server), a combined movie/TV
  add-on, a Python add-on (listed as unsupported) and `metadata.local` (skipped). `pages/` holds
  the made-up pages the sample scrapers read (search results, a film, a show, its episode list and
  one episode, and a page whose markup the scraper does not know).
- `ratings/`: `mdblist_heat.json` (MDBList's `ratings` array in its documented shape, with sources the
  app has no key for, a null value and null vote counts), `mdblist_invalid_key.json`,
  `mdblist_not_found.json`, and `title.ratings.tsv`, a six-line stand-in for the IMDb ratings
  dataset (the real file is downloaded on demand and never committed; tests gzip this one).
