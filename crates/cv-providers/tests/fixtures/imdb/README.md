# IMDb fixtures

Hand-built pages in the shape of IMDb's Next.js pages: an HTML document whose
`<script id="__NEXT_DATA__">` block holds `props.pageProps`. Only the fields the provider reads
are present. Titles, names and image hosts are made up; nothing here is copied from the site.

- `title_movie.html`, `title_show.html`: title pages (`aboveTheFoldData`, `mainColumnData`).
- `find_results.html`, `find_empty.html`: the `find` page (`titleResults.results`).
- `title_changed.html`: valid `__NEXT_DATA__` with a different structure ("page format changed").
- `title_no_data.html`: a human-check page with no `__NEXT_DATA__`.
- `title_bad_json.html`: a `__NEXT_DATA__` block that is not valid JSON.
- `chart_top.html`, `chart_toptv.html`: the Top 250 chart pages (`pageData.chartTitles.edges`, 250 made-up entries each).
- `chart_changed.html`: a chart page with valid `__NEXT_DATA__` in a different structure.
- `mediaindex_poster.html`, `mediaindex_still_frame.html`: the image index pages
  (`contentData.data.title.all_images.edges[].node` with `url`, `width`, `height`): portrait and
  landscape pictures in each, a repeated image and an entry without a URL.
  `mediaindex_empty.html` (a title with no images) and `mediaindex_changed.html` (valid
  `__NEXT_DATA__` without an image list: "page format changed").
