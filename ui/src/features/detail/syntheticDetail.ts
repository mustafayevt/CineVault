import type { MovieDetail, MovieDetailBackend, NfoPreview } from "./detailBackend";
import { syntheticLifecycle, syntheticScrapedLifecycle } from "../lifecycle/syntheticLifecycle";
import { DetailRequestError } from "./detailBackend";

/**
 * A fixture movie for a plain browser (no desktop backend) and for tests. It is
 * a 4K Dolby Vision title in a collection with six
 * ratings, a stream table, five of eight artwork slots filled and three trailers.
 */

// ThumbHashes of gradients (produced by the Rust encoder) so the backdrop and posters show a
// preview where there is no image to load.
const BACKDROP_HASH = "d1690a2d0888887f7787778777788887887f87f887";
const POSTER_HASH = "8ce705050088887f78878787877887877680770778";
const LOGO_HASH = "4db6091d0688887f77877788777888877780770778";

const KEY = (n: number): string => n.toString(16).padStart(32, "0");

const DAY_MS = 86_400_000;

/** When the sample ratings were "fetched". */
const FETCHED_AT = Date.UTC(2026, 9, 20);

/** The id of the fixture movie; any other id is "not found". */
export const SYNTHETIC_MOVIE_ID = "1";

/**
 * Whether the fixture title has its files. A page opened with `?filesMissing=1` shows the
 * missing-files state, so screenshots can check the banner without a real library.
 */
export function syntheticAvailability(): string {
  return typeof location !== "undefined" && location.search.includes("filesMissing=1")
    ? "missing"
    : "online";
}

/**
 * The fixture movie with the ratings of a title in the middle of its scrape: TMDB has its value,
 * IMDb is being fetched, two sources need a key, one needs a connection, one found nothing and
 * one failed. The browser shows it for movie 2.
 */
function syntheticMixedRatings(movie: MovieDetail): MovieDetail {
  const chip = (key: string, state: string, reason: string, provider: string | null = null) => ({
    key,
    state,
    reason,
    provider,
  });
  return {
    ...movie,
    ratings: movie.ratings.filter((rating) => rating.source === "tmdb"),
    rating_chips: [
      chip("tmdb", "value", ""),
      chip("imdb", "fetching", "The IMDb rating is being fetched."),
      chip("tomatometerallcritics", "needs_key", "Rotten Tomatoes needs an OMDb key.", "omdb"),
      chip("metacritic", "needs_key", "Metacritic needs an OMDb key.", "omdb"),
      chip("trakt", "connect_service", "Connect Trakt to show its community rating.", "trakt"),
      chip("letterboxd", "not_found", "Letterboxd has no rating for this title."),
      chip("myanimelist", "failed", "MyAnimeList did not answer."),
    ],
  };
}

/** The id of the fixture movie with the longest real-world first screen (`syntheticLongFacts`). */
const SYNTHETIC_LONG_FACTS_ID = "3";

/**
 * The fixture movie as a fully scraped library title with the most to show on its first screen:
 * a clear logo, every rating source with a value, a long plot, many tags and the long facts of
 * Blade Runner (several writers, four studios, three countries, an edition, a collection, a play
 * history). The browser shows it for movie 3; the screenshot check `check:facts` requires its
 * key facts on the first screen at 1366x768 and 1440x900 (library-browsing "Important facts
 * visible on open").
 */
function syntheticLongFacts(movie: MovieDetail): MovieDetail {
  // The pages MDBList names for some sources are stored with their ratings.
  const rating = (source: string, value: number, max: number, url: string | null = null) => ({
    source,
    value,
    max,
    votes: 0,
    fetched_at_ms: FETCHED_AT,
    url,
  });
  const ratings = [
    rating("tmdb", 7.9, 10),
    rating("imdb", 8.1, 10),
    rating("trakt", 8.2, 10),
    rating("letterboxd", 4.2, 5, "https://letterboxd.com/film/blade-runner/"),
    rating("tomatometerallcritics", 89, 100, "https://www.rottentomatoes.com/m/blade_runner"),
    rating("metacritic", 84, 100, "https://www.metacritic.com/movie/blade-runner/"),
    rating("tomatometerallaudience", 91, 100, "https://www.rottentomatoes.com/m/blade_runner"),
    rating("rogerebert", 4, 4, "https://www.rogerebert.com/reviews/blade-runner-1982-1"),
    rating("metacriticuser", 8.6, 10),
    rating("tomatometeravgcritics", 8.5, 10),
  ];
  return {
    ...movie,
    title: "Blade Runner",
    original_title: "Blade Runner",
    tagline: "Man has made his match... now it's his problem.",
    year: 1982,
    certification: "R",
    runtime_minutes: 117,
    release_date: "1982-06-25",
    genres: ["Science Fiction", "Drama", "Thriller"],
    directors: ["Ridley Scott"],
    writers: ["Hampton Fancher", "David Webb Peoples", "Philip K. Dick"],
    studios: [
      "The Ladd Company",
      "Shaw Brothers",
      "Warner Bros. Pictures",
      "Blade Runner Partnership",
    ],
    countries: ["US", "HK", "GB"],
    original_language: "en",
    edition: "The Final Cut",
    plot: "In the smog-choked dystopian Los Angeles of 2019, blade runner Rick Deckard is called out of retirement to terminate a quartet of replicants who have escaped to Earth seeking their creator for a way to extend their short life spans. As he hunts them through the rain-soaked streets, the neon markets and the decaying Bradbury Building, Deckard falls for Rachael, an experimental replicant who does not know what she is, and begins to doubt what separates the hunter from the hunted.",
    collection: { id: "901", name: "Blade Runner Collection", owned: 2, total: 2 },
    tags: [
      "cyberpunk",
      "neo-noir",
      "dystopia",
      "android",
      "artificial intelligence",
      "los angeles, california",
      "flying car",
      "genetic engineering",
      "based on novel or book",
      "future noir",
    ],
    ratings,
    rating_chips: ratings.map(({ source }) => ({
      key: source,
      state: "value",
      reason: "",
      provider: null,
    })),
    user_rating: null,
    watched: true,
    play_count: 3,
    last_watched_ms: FETCHED_AT,
    source_label: "Movies 4K Archive",
    lifecycle: syntheticScrapedLifecycle(
      { index: 3, title: "Blade Runner", year: 1982, unmatched: false, needsReview: false },
      false,
    ),
    logo: { key: KEY(3), thumbhash: LOGO_HASH, ambient: null },
    hero_audio: {
      index: 0,
      label: "DTS-HD MA 5.1",
      codec: "DTS-HD MA",
      channels: 6,
      language: "en",
      title: "",
      flags: ["default"],
    },
  };
}

/** The id of the fixture movie that has no artwork and no match yet (`syntheticNoArtwork`). */
const SYNTHETIC_NO_ARTWORK_ID = "4";

/**
 * A title the scan found and nothing matched yet: no poster, backdrop or logo, no plot, no
 * ratings, a long folder name for a title. The browser shows it for movie 4; its hero is one
 * column at every size (QA final-01 found the text squeezed into the poster's column).
 */
function syntheticNoArtwork(movie: MovieDetail): MovieDetail {
  return {
    ...movie,
    title: "Крёстный отец Part II Extended Director's Cut",
    original_title: "",
    tagline: "",
    plot: "",
    tags: [],
    genres: [],
    directors: [],
    writers: [],
    studios: [],
    countries: [],
    collection: null,
    edition: "",
    certification: "",
    ratings: [],
    rating_chips: [{ key: "tmdb", state: "not_fetched", reason: "", provider: null }],
    poster: null,
    backdrop: null,
    logo: null,
    // Nothing matched it: it is New and "Not matched", so the hero offers Scrape.
    lifecycle: syntheticLifecycle({
      index: 4,
      title: "Крёстный отец",
      year: 1974,
      unmatched: true,
      needsReview: true,
    }),
    ids: [],
  };
}

type Stream = MovieDetail["videos"][number]["streams"][number];

const NO_STREAM: Stream = {
  kind: "audio",
  codec: "",
  width: 0,
  height: 0,
  frame_rate: null,
  bitrate_kbps: 0,
  bit_depth: 0,
  sample_rate_hz: 0,
  hdr: [],
  hdr_details: null,
  channels: 0,
  language: "",
  title: "",
  flags: [],
};

export function syntheticMovieDetail(now: number = Date.now()): MovieDetail {
  const base = "Dune Part Two (2024) [2160p] [WEB-DL] [DV HDR10] [TrueHD Atmos 7.1]";
  const folder = `D:\\Media\\Movies\\${base}`;
  const image = (n: number, thumbhash: string) => ({ key: KEY(n), thumbhash, ambient: null });
  const slot = (
    name: string,
    file: string | null,
    n: number,
    hash: string,
    width: number,
    height: number,
    size: number,
  ) => ({
    slot: name,
    file_name: file,
    image: file === null ? null : image(n, hash),
    width: file === null ? 0 : width,
    height: file === null ? 0 : height,
    size_bytes: file === null ? 0 : size,
    online: file === null,
  });
  const streams: Stream[] = [
    {
      ...NO_STREAM,
      kind: "video",
      codec: "HEVC",
      width: 3840,
      height: 1608,
      frame_rate: 23.976,
      bitrate_kbps: 46_200,
      bit_depth: 10,
      hdr: ["Dolby Vision", "HDR10"],
      hdr_details: {
        dolby_vision_profile: "8.1",
        dolby_vision_compatibility: "HDR10",
        mastering_primaries: "Display P3",
        mastering_min_nits: 0.005,
        mastering_max_nits: 1000,
        max_cll_nits: 1000,
        max_fall_nits: 400,
      },
    },
    {
      ...NO_STREAM,
      codec: "TrueHD Atmos",
      bit_depth: 24,
      sample_rate_hz: 48_000,
      channels: 8,
      language: "en",
      flags: ["default"],
    },
    {
      ...NO_STREAM,
      codec: "E-AC-3",
      bitrate_kbps: 640,
      channels: 6,
      language: "en",
      title: "Compatibility",
    },
    { ...NO_STREAM, kind: "subtitle", codec: "PGS", language: "en", flags: ["forced"] },
    {
      ...NO_STREAM,
      kind: "subtitle",
      codec: "SubRip",
      language: "en",
      flags: ["hearing_impaired"],
    },
    { ...NO_STREAM, kind: "subtitle", codec: "SubRip", language: "tr" },
  ];
  const actors: ReadonlyArray<readonly [string, string]> = [
    ["Timothée Chalamet", "Paul Atreides"],
    ["Zendaya", "Chani"],
    ["Rebecca Ferguson", "Lady Jessica"],
    ["Javier Bardem", "Stilgar"],
    ["Josh Brolin", "Gurney Halleck"],
    ["Austin Butler", "Feyd-Rautha"],
    ["Florence Pugh", "Princess Irulan"],
    ["Stellan Skarsgård", "Baron Harkonnen"],
  ];
  return {
    id: SYNTHETIC_MOVIE_ID,
    title: "Dune: Part Two",
    original_title: "",
    tagline: "Long live the fighters.",
    year: 2024,
    certification: "PG-13",
    runtime_minutes: 166,
    release_date: "2024-03-01",
    genres: ["Science Fiction", "Adventure"],
    directors: ["Denis Villeneuve"],
    writers: ["Denis Villeneuve", "Jon Spaihts"],
    studios: ["Legendary Pictures"],
    countries: ["US"],
    languages: ["English"],
    edition: "Theatrical Edition",
    media_source: "Web-DL",
    plot: "Paul Atreides joins Chani and the Fremen in the deep desert, torn between revenge on the houses that destroyed his family and a prophecy he fears will set the universe on fire.",
    collection: { id: "900", name: "Dune Collection", owned: 2, total: 2 },
    ids: [
      { provider: "tmdb", value: "693134" },
      { provider: "imdb", value: "tt15239678" },
    ],
    ratings: [
      { source: "imdb", value: 8.5, max: 10, votes: 640_000, fetched_at_ms: FETCHED_AT },
      { source: "tomatometerallcritics", value: 92, max: 100, votes: 0, fetched_at_ms: FETCHED_AT },
      { source: "metacritic", value: 79, max: 100, votes: 0, fetched_at_ms: 0 },
      { source: "letterboxd", value: 4.4, max: 5, votes: 0, fetched_at_ms: FETCHED_AT },
      { source: "tmdb", value: 8.1, max: 10, votes: 5_400, fetched_at_ms: FETCHED_AT },
      { source: "trakt", value: 8.5, max: 10, votes: 0, fetched_at_ms: FETCHED_AT },
    ],
    rating_chips: [
      "imdb",
      "tomatometerallcritics",
      "metacritic",
      "letterboxd",
      "tmdb",
      "trakt",
    ].map((key) => ({ key, state: "value", reason: "", provider: null })),
    user_rating: null,
    top250: 0,
    watched: false,
    play_count: 0,
    last_watched_ms: null,
    availability: syntheticAvailability(),
    locked: false,
    needs_review: false,
    // Scraped, with one rename waiting in Organize.
    lifecycle: syntheticScrapedLifecycle(
      { index: 3, title: "Dune: Part Two", year: 2024, unmatched: false, needsReview: false },
      false,
    ),
    is_new: false,
    date_added_ms: now - 3 * DAY_MS,
    folder,
    data_source: "D:\\Media\\Movies",
    source_label: "Movies",
    note: "",
    tags: [],
    backdrop: image(1, BACKDROP_HASH),
    logo: null,
    poster: image(2, POSTER_HASH),
    tech: [
      { label: "2160p", facet: "video_format", values: ["2160p"], number: null, notable: false },
      {
        label: "Dolby Vision Profile 8.1",
        facet: "hdr_format",
        values: ["Dolby Vision"],
        number: null,
        notable: true,
      },
      { label: "HDR10", facet: "hdr_format", values: ["HDR10"], number: null, notable: false },
      {
        label: "HEVC 10-bit",
        facet: "video_codec",
        values: ["HEVC"],
        number: null,
        notable: false,
      },
      { label: "46 Mb/s", facet: "video_bitrate", values: [], number: 46_000, notable: false },
      {
        label: "TrueHD Atmos 7.1",
        facet: "audio_codec",
        values: ["TrueHD Atmos"],
        number: null,
        notable: true,
      },
      { label: "2.39:1", facet: "aspect_ratio", values: ["2.39"], number: null, notable: false },
      { label: "Web-DL", facet: "media_source", values: ["WEB_DL"], number: null, notable: false },
    ],
    original_language: "en",
    hero_audio: {
      index: 0,
      label: "TrueHD Atmos 7.1",
      codec: "TrueHD Atmos",
      channels: 8,
      language: "en",
      title: "",
      flags: ["default"],
    },
    credits: [
      ...actors.map(([name, role], index) => ({
        name,
        role,
        kind: "actor",
        portrait: null,
        imdb_id: index === 0 ? "nm0000001" : null,
        tmdb_id: index === 0 ? "1234" : null,
      })),
      {
        name: "Denis Villeneuve",
        role: "",
        kind: "director",
        portrait: null,
        imdb_id: "nm0898288",
        tmdb_id: "137427",
      },
      {
        name: "Jon Spaihts",
        role: "Screenplay",
        kind: "writer",
        portrait: null,
        imdb_id: null,
        tmdb_id: null,
      },
      {
        name: "Hans Zimmer",
        role: "Original Music Composer",
        kind: "composer",
        portrait: null,
        imdb_id: "nm0001877",
        tmdb_id: "947",
      },
    ],
    videos: [
      {
        name: `${base}.mkv`,
        folder,
        size_bytes: 58_400_000_000,
        container: "Matroska",
        duration_secs: 166 * 60,
        analysed: true,
        read_problem: "",
        streams,
      },
    ],
    files: [
      { kind: "nfo", name: `${base}.nfo`, size_bytes: 6_000 },
      { kind: "poster", name: "poster.jpg", size_bytes: 1_100_000 },
      { kind: "fanart", name: "fanart.jpg", size_bytes: 2_400_000 },
      { kind: "clearlogo", name: "clearlogo.png", size_bytes: 88_000 },
      { kind: "trailer", name: "Dune Part Two (2024)-trailer.mkv", size_bytes: 212_000_000 },
      { kind: "subtitle", name: "Dune Part Two (2024).tr.srt", size_bytes: 94_000 },
    ],
    artwork: [
      slot("poster", "poster.jpg", 2, POSTER_HASH, 2000, 3000, 1_100_000),
      slot("fanart", "fanart.jpg", 1, BACKDROP_HASH, 3840, 2160, 2_400_000),
      slot("clearlogo", "clearlogo.png", 3, LOGO_HASH, 800, 310, 88_000),
      slot("thumb", "thumb.jpg", 4, BACKDROP_HASH, 1920, 1080, 300_000),
      slot("keyart", "keyart.jpg", 5, POSTER_HASH, 2000, 3000, 900_000),
      slot("banner", null, 0, "", 0, 0, 0),
      slot("clearart", null, 0, "", 0, 0, 0),
      slot("disc", null, 0, "", 0, 0, 0),
    ],
    trailers: [
      {
        name: "Official Trailer 3",
        provider: "tmdb",
        quality: "2160p",
        language: "en",
        url: "https://example.test/trailer-3",
        local_file: "Dune Part Two (2024)-trailer.mkv",
      },
      {
        name: "Official Trailer 2",
        provider: "tmdb",
        quality: "1080p",
        language: "en",
        url: "https://example.test/trailer-2",
        local_file: null,
      },
      {
        name: "Teaser",
        provider: "imdb",
        quality: "1080p",
        language: "en",
        url: "https://example.test/teaser",
        local_file: null,
      },
    ],
    subtitles: [
      {
        file_name: null,
        language: "en",
        codec: "PGS",
        title: "",
        flags: ["forced"],
        video: `${base}.mkv`,
        position: 0,
        codec_id: "S_HDMV/PGS",
      },
      {
        file_name: null,
        language: "en",
        codec: "SubRip",
        title: "",
        flags: ["hearing_impaired"],
        video: `${base}.mkv`,
        position: 1,
        codec_id: "S_TEXT/UTF8",
      },
      {
        file_name: null,
        language: "tr",
        codec: "SubRip",
        title: "",
        flags: [],
        video: `${base}.mkv`,
        position: 2,
        codec_id: "S_TEXT/UTF8",
      },
      {
        file_name: "Dune Part Two (2024).tr.srt",
        language: "tr",
        codec: "SubRip",
        title: "",
        flags: [],
        video: null,
        position: null,
        codec_id: "",
      },
    ],
  };
}

export function syntheticNfo(): NfoPreview {
  const text = [
    '<?xml version="1.0" encoding="utf-8" standalone="yes"?>',
    "<!--created by CineVault for Kodi-->",
    "<movie>",
    "  <title>Dune: Part Two</title>",
    "  <year>2024</year>",
    "  <ratings>",
    '    <rating name="themoviedb" max="10" default="true"><value>8.1</value><votes>5400</votes></rating>',
    '    <rating name="imdb" max="10"><value>8.5</value><votes>640000</votes></rating>',
    "  </ratings>",
    '  <uniqueid type="tmdb" default="true">693134</uniqueid>',
    '  <uniqueid type="imdb">tt15239678</uniqueid>',
    "  <genre>Science Fiction</genre>",
    "  <genre>Adventure</genre>",
    "  <director>Denis Villeneuve</director>",
    "  <set>",
    "    <name>Dune Collection</name>",
    "  </set>",
    "  <premiered>2024-03-01</premiered>",
    "  <runtime>166</runtime>",
    "  <mpaa>PG-13</mpaa>",
    "</movie>",
  ].join("\n");
  return {
    text,
    format: "Kodi",
    path: "D:\\Media\\Movies\\Dune Part Two (2024)\\Dune Part Two (2024).nfo",
    sync: "in_sync",
    // The file on disk says the same.
    file_text: text,
  };
}

/** The NFO of the fixture movie when the file on disk is an older one (the runtime and the certification differ). */
export function syntheticStaleNfo(): NfoPreview {
  const fresh = syntheticNfo();
  return {
    ...fresh,
    sync: "differs",
    file_text: fresh.text
      .replace("<runtime>166</runtime>", "<runtime>165</runtime>")
      .replace("<mpaa>PG-13</mpaa>", "<mpaa>Rated PG-13</mpaa>"),
  };
}

/** A backend over the fixture movie. Watched changes are kept for the session. */
export function createSyntheticDetailBackend(): MovieDetailBackend {
  let watched = false;
  let playCount = 0;
  const find = (id: string): MovieDetail => {
    // The fixture stands in for every movie of the sample library (ids are 1, 2, 3, ...), so the
    // Inspector and the page have something to show whichever title is chosen.
    if (!/^[1-9]\d*$/.test(id)) {
      throw new DetailRequestError("not_found", "That movie is not in the library any more.");
    }
    const movie = { ...syntheticMovieDetail(), id, watched, play_count: playCount };
    if (id === "2") return syntheticMixedRatings(movie);
    if (id === SYNTHETIC_NO_ARTWORK_ID) return syntheticNoArtwork(movie);
    return id === SYNTHETIC_LONG_FACTS_ID ? syntheticLongFacts(movie) : movie;
  };
  return {
    movie: (id) => Promise.resolve().then(() => find(id)),
    nfo: (id) =>
      Promise.resolve().then(() => {
        find(id);
        return syntheticNfo();
      }),
    setWatched: (id, next) =>
      Promise.resolve().then(() => {
        find(id);
        if (watched === next) return [];
        const previous = [{ id, watched, play_count: playCount, last_watched_ms: null }];
        watched = next;
        playCount = next ? Math.max(playCount, 1) : 0;
        return previous;
      }),
    restoreWatched: (previous) =>
      Promise.resolve().then(() => {
        const state = previous.find((s) => s.id === SYNTHETIC_MOVIE_ID);
        if (state === undefined) return;
        watched = state.watched;
        playCount = state.play_count;
      }),
    // A browser has no profile pictures to fetch and no provider to ask.
    castPortraits: () => Promise.resolve([]),
    personImdbId: () => Promise.resolve(null),
  };
}
