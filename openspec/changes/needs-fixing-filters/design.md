## Context

The library query already carries `facets`: chips of the filter catalogue (`cv_db::repo::facet`), ANDed with the quick-search chips, and `library_facet_values` returns a list filter's values with how many titles have each under the query's other chips. The Library UI sends no facets yet; its filter row has the stage chips and the Drive, Quality and Problems dropdowns.

## Decisions

- **One dropdown chip, not one chip per problem.** Five more chips would not fit the row beside the stage chips, the three dropdowns and the footer at 1366 px. One "Needs fixing" chip with checkable entries stays discoverable, keeps the row's rhythm, and its menu has room for a count per entry. The menu stays open while entries are ticked, like the Columns menu.
- **Catalogue underneath.** Poster, Fan art and Clear logo are values of `missing_artwork`; Plot and Title are values of `missing_metadata`. The UI sends one facet per filter id, mode ALL, so ticking several entries lists the titles missing all of them. Both filters already count a show by its own `media_file` rows and its own columns, never by episodes, so TV needs no extra SQL.
- **Counts from the catalogue.** Each entry's number is the filter's value count for the current view (`library_facet_values` on the query without the Needs fixing choices): "No poster 12" is what ticking No poster alone would list. The counts refresh with the rest of the library queries. Not scraped uses the pipeline count of the New stage, like the stage chip.
- **Not scraped is the New stage.** A title is scraped when the lifecycle says so; the stage chip already filters it. The entry ticks and unticks the New stage chip (and shows ticked when it is on), so there is one state and no second filter that could disagree with the chips.
- **Title is the literal empty check.** The scan names a never-scraped title after its folder or file, so it is rarely empty; `trim(title) = ''` finds the odd empty one. It is added to the start of `missing_metadata`'s list as "Title", counted like the other entries.
- **Choices are per library and not persisted,** like every other filter: `LibraryFilters.fix`, reset by "Clear filters and search".
- **Selection and batches unchanged.** The listing is the one query, so Ctrl+A reads every id of the result and the selection bar's actions receive them; Scrape and Artwork already take several titles (`/scrape?ids=`, `/artwork?ids=` opens the batch picker).
- **Empty result.** When Needs fixing is ticked and nothing is listed the list says "Nothing to fix here", with a button that clears the Needs fixing choices only, so the other filters and the search stay.

## Risks

- The `missing_artwork` source is a cross join of the items with eight artwork types; the counts run it for two filters each time the view changes. It is covered by the catalogue's speed test; counts are cached for 30 seconds and read once per view.

## Safety

The change reads the library and writes nothing to media folders or settings. The actions reached from the selection bar keep their own previews and Undo.
