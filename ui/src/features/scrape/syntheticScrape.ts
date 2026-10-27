import {
  createFakeProviderTransport,
  perSearchTransport,
} from "../workspaces/fakeProviderTransport";
import type { FakeProviderTransport } from "../workspaces/fakeProviderTransport";
import { SYNTHETIC_MOVIE_ID } from "../detail/syntheticDetail";
import {
  PREVIEW_DETAILS_SOURCE,
  PREVIEW_HIT_SOURCE,
  SEARCH_PROVIDER,
  localFieldSelection,
} from "./scrapeBackend";
import type {
  ScrapeApplied,
  SearchOnlyResult,
  ScrapeBackend,
  ScrapeFieldDiff,
  ScrapeKind,
  ScrapeRequest,
  ScrapeStreamHit,
  ScrapeTarget,
} from "./scrapeBackend";

/** Fixtures for a plain browser and for tests: a few titles, two of them already in the library. */

interface Title {
  kind: ScrapeKind;
  id: string;
  title: string;
  year: number;
  director: string;
  runtime: number;
  overview: string;
  genres: string[];
  rating: number;
  votes: number;
  cast: [string, string][];
  imdb: string;
  /** The title in its original language, when it differs. */
  original?: string;
  /** The library item that already holds the title. */
  inLibrary?: { itemId: string; name: string };
  /** The TMDB id, for a pasted TMDB link. */
  tmdb?: string;
  /** How sure the search is of this title, 0 to 1; the fixtures default to just under 0.9. */
  confidence?: number;
}

const TITLES: readonly Title[] = [
  {
    kind: "movie",
    id: "heat-1995",
    tmdb: "949",
    title: "Heat",
    year: 1995,
    director: "Michael Mann",
    runtime: 170,
    overview:
      "A group of professional bank robbers start to feel the heat from police when they unknowingly leave a clue at their latest heist.",
    genres: ["Crime", "Drama", "Thriller"],
    rating: 7.9,
    votes: 9230,
    cast: [
      ["Al Pacino", "Vincent Hanna"],
      ["Robert De Niro", "Neil McCauley"],
      ["Val Kilmer", "Chris Shiherlis"],
    ],
    imdb: "tt0113277",
  },
  {
    kind: "movie",
    id: "heat-1986",
    tmdb: "24081",
    title: "Heat",
    year: 1986,
    director: "Dick Richards",
    runtime: 101,
    overview:
      "A Las Vegas bodyguard with a gambling problem takes on the mob after a friend is assaulted.",
    genres: ["Action", "Thriller"],
    rating: 5.8,
    votes: 410,
    cast: [
      ["Burt Reynolds", "Nick Escalante"],
      ["Karen Young", "Holly"],
    ],
    imdb: "tt0093164",
  },
  {
    kind: "movie",
    id: "solaris-1972",
    title: "Solaris",
    original: "Solyaris",
    year: 1972,
    director: "Andrei Tarkovsky",
    runtime: 167,
    overview:
      "A psychologist is sent to a space station orbiting an ocean planet that seems to answer the crew’s memories.",
    genres: ["Drama", "Science Fiction", "Mystery"],
    rating: 8.0,
    votes: 7421,
    cast: [
      ["Donatas Banionis", "Kris Kelvin"],
      ["Natalya Bondarchuk", "Hari"],
      ["Jüri Järvet", "Dr. Snaut"],
    ],
    imdb: "tt0069293",
  },
  {
    kind: "movie",
    id: "solaris-2002",
    title: "Solaris",
    year: 2002,
    director: "Steven Soderbergh",
    runtime: 99,
    overview:
      "A grieving psychologist travels to a station above Solaris and meets someone who should not exist.",
    genres: ["Drama", "Science Fiction", "Romance"],
    rating: 6.2,
    votes: 3110,
    cast: [
      ["George Clooney", "Chris Kelvin"],
      ["Natascha McElhone", "Rheya"],
      ["Jeremy Davies", "Snow"],
    ],
    imdb: "tt0307987",
    inLibrary: { itemId: "m-solaris-2002", name: "Solaris (2002)" },
  },
  {
    kind: "movie",
    id: "dune-1984",
    title: "Dune",
    year: 1984,
    director: "David Lynch",
    runtime: 137,
    overview: "A noble family accepts control of a desert planet and its priceless spice.",
    genres: ["Science Fiction", "Adventure"],
    rating: 6.3,
    votes: 4980,
    cast: [
      ["Kyle MacLachlan", "Paul Atreides"],
      ["Francesca Annis", "Lady Jessica"],
    ],
    imdb: "tt0087182",
  },
  {
    kind: "movie",
    id: "dune-2021",
    title: "Dune",
    year: 2021,
    director: "Denis Villeneuve",
    runtime: 155,
    overview: "Paul Atreides travels to the most dangerous planet in the universe.",
    genres: ["Science Fiction", "Adventure"],
    rating: 7.8,
    votes: 12834,
    cast: [
      ["Timothée Chalamet", "Paul Atreides"],
      ["Rebecca Ferguson", "Lady Jessica"],
    ],
    imdb: "tt1160419",
    inLibrary: { itemId: "m-dune-2021", name: "Dune (2021)" },
  },
  {
    kind: "show",
    id: "severance-2022",
    title: "Severance",
    year: 2022,
    director: "Ben Stiller",
    runtime: 50,
    overview: "Employees agree to split their memories between their work and personal lives.",
    genres: ["Drama", "Mystery"],
    rating: 8.7,
    votes: 9020,
    cast: [["Adam Scott", "Mark Scout"]],
    imdb: "tt11280740",
  },
];

function words(text: string): string[] {
  return text
    .toLowerCase()
    .replace(/\(\d{4}\)/g, " ")
    .split(/[^\p{L}\p{N}]+/u)
    .filter((word) => word !== "");
}

/** A title that finds itself (confident) and a lookalike (not), for a query the fixtures lack. */
function genericTitles(query: string, kind: ScrapeKind): { title: Title; index: number }[] {
  const year = Number(/\((\d{4})\)/.exec(query)?.[1] ?? "2019");
  const name = query
    .replace(/\(\d{4}\)/g, " ")
    .split(/\s+/)
    .filter((word) => word !== "")
    .map((word) => word.charAt(0).toUpperCase() + word.slice(1))
    .join(" ");
  const base = {
    kind,
    director: "Sample Director",
    runtime: 118,
    overview: `A sample story for ${name}, shown because this browser has no providers.`,
    genres: ["Drama"],
    rating: 7.1,
    votes: 1200,
    cast: [["Sample Actor", "Lead"]] as [string, string][],
  };
  return [
    {
      title: {
        ...base,
        id: `generic-${name}-${year}`,
        title: name,
        year,
        imdb: "tt0000001",
        confidence: 0.97,
      },
      index: 100,
    },
    {
      title: {
        ...base,
        id: `generic-the-${name}-${year - 11}`,
        title: `The ${name}`,
        year: year - 11,
        runtime: 96,
        imdb: "tt0000002",
        confidence: 0.31,
      },
      index: 101,
    },
  ];
}

function streamHitOf(title: Title, index: number, providers: readonly string[]): ScrapeStreamHit {
  const confidence = title.confidence ?? Math.min(1, 0.9 - title.year / 1e6);
  return {
    id: `hit-${index + 1}`,
    candidate: {
      provider: providers[0] ?? "tmdb",
      providerId: title.id,
      title: title.title,
      originalTitle: title.original ?? null,
      year: title.year,
      director: title.director,
      runtimeMinutes: title.runtime,
      language: null,
      overview: title.overview,
      posterKey: null,
      confidence,
      rank: confidence,
      evidence: [],
      breakdown: { similarity: confidence, yearPenalty: 0, posterPenalty: 0, evidence: 0 },
    },
    providers: providers.map((provider) => ({
      provider,
      providerId: provider === "omdb" ? title.imdb : title.id,
      confidence,
    })),
    ids: [`imdb:${title.imdb}`],
    inLibrary: title.inLibrary
      ? { itemId: title.inLibrary.itemId, name: title.inLibrary.name }
      : null,
    absorbs: [],
  };
}

/** The titles the fixtures find for a query, in order. */
function findTitles(
  query: string,
  kind: ScrapeKind,
  generic: boolean,
): { title: Title; index: number }[] {
  const wanted = words(query);
  // An IMDb ID finds one title exactly.
  const shortcut = /\btt\d{7,9}\b/i.exec(query)?.[0]?.toLowerCase();
  // A TMDB link (`themoviedb.org/movie/949`) or `tmdb:949` finds one title exactly, whatever its kind.
  const tmdbLink = /themoviedb\.org\/(?:movie|tv)\/(\d+)|\btmdb[:\s](\d+)/i.exec(query);
  const tmdbId = tmdbLink?.[1] ?? tmdbLink?.[2];
  const matches = TITLES.map((title, index) => ({ title, index })).filter(({ title }) => {
    if (tmdbId !== undefined) return title.tmdb === tmdbId;
    if (title.kind !== kind) return false;
    if (shortcut !== undefined) return title.imdb === shortcut;
    return (
      wanted.length > 0 &&
      // A title is found by a query that is part of its name or that contains the whole name.
      (wanted.every((word) => words(title.title).includes(word)) ||
        words(title.title).every((word) => wanted.includes(word)))
    );
  });
  // Any other title the fixtures do not know finds itself and a lookalike, so a plain browser
  // shows scores for every title of its library. A search that starts with "zz" finds nothing.
  const lookalikes =
    generic &&
    matches.length === 0 &&
    tmdbId === undefined &&
    shortcut === undefined &&
    wanted.length > 0 &&
    !query.trim().toLowerCase().startsWith("zz")
      ? genericTitles(query, kind)
      : [];
  return matches.length > 0 ? matches : lookalikes;
}

/**
 * The synthetic backend's streaming search: OMDb answers first, TMDB later finds the same titles
 * (so each entry is sent again with both providers), and TheTVDB waits for a key. A search that
 * matches nothing says why, as the real one does, just before it is done.
 */
export function createSyntheticScrapeStream(
  query: string,
  kind: ScrapeKind = "movie",
  generic = false,
): FakeProviderTransport<ScrapeStreamHit> {
  const found = findTitles(query, kind, generic);
  const first = found.map(({ title, index }) => streamHitOf(title, index, ["omdb"]));
  const both = found.map(({ title, index }) => streamHitOf(title, index, ["omdb", "tmdb"]));
  return createFakeProviderTransport<ScrapeStreamHit>(
    {
      omdb: { delayMs: 100, items: first },
      tmdb: { delayMs: 300, items: both },
      tvdb: { delayMs: 0, needsKey: true },
    },
    {
      beforeDone: () =>
        found.length > 0
          ? null
          : {
              kind: "failed",
              provider: SEARCH_PROVIDER,
              reason: `No provider knows a title called “${query.trim()}”. Try a different search.`,
              retryable: false,
            },
    },
  );
}

const row = (
  field: string,
  group: string,
  current: string | null,
  incoming: string | null,
  change: ScrapeFieldDiff["change"],
  locked = false,
): ScrapeFieldDiff => ({
  field,
  group,
  current,
  incoming,
  change,
  locked,
  incomingImageKey: null,
});

/**
 * The synthetic backend's streaming preview of a chosen result: the diff of title, year and
 * runtime at once (source `hit`), then every field when the "details" arrive. The plot is
 * locked, as a field the user locked would be.
 */
export function createSyntheticScrapePreview(): FakeProviderTransport<ScrapeFieldDiff> {
  const hit = [
    row("title", "text", "solaris dvdrip", "Solaris", "changed"),
    row("year", "text", null, "1972", "added"),
    row("runtime", "text", "99", "167", "changed"),
  ];
  const details = [
    ...hit,
    row("plot", "text", "My own plot", "A psychologist is sent to a station.", "changed", true),
    row("tagline", "text", null, "The ocean remembers.", "added"),
    row("genres", "text", "Drama", "Drama, Science Fiction", "changed"),
    row("studio", "text", "Mosfilm", "Mosfilm", "same"),
    row("actors", "people", null, "Donatas Banionis, Natalya Bondarchuk", "added"),
    row("rating", "ratings-and-ids", "7.9 imdb", "8.0 imdb", "changed"),
    row(
      "poster",
      "artwork",
      "https://old.example/p.jpg",
      "https://img.example/solaris.jpg",
      "changed",
    ),
    row("fanart", "artwork", null, "https://img.example/solaris-bg.jpg", "added"),
  ];
  return createFakeProviderTransport<ScrapeFieldDiff>({
    [PREVIEW_HIT_SOURCE]: { delayMs: 0, items: hit },
    [PREVIEW_DETAILS_SOURCE]: { delayMs: 300, items: details },
  });
}

/** The search-only pass of the fixtures: the same titles as the stream, scored, nothing written. */
function syntheticSearchOnly(query: string, kind: ScrapeKind, generic: boolean): SearchOnlyResult {
  const hits = findTitles(query, kind, generic)
    .map(({ title, index }) => streamHitOf(title, index, ["tmdb"]))
    .sort((a, b) => (b.candidate.confidence ?? 0) - (a.candidate.confidence ?? 0));
  const [top, second] = hits;
  const confident =
    top !== undefined &&
    top.candidate.confidence !== null &&
    top.candidate.confidence >= 0.9 &&
    (second === undefined || top.candidate.confidence !== second.candidate.confidence);
  return { hits, confident };
}

/**
 * The match of a fixture title that is already Scraped (the sample library's first movie, Dune:
 * Part Two), so a plain browser shows the "Current match" row pinned on its picker. Only the
 * browser fallback uses it; the test backends choose their own.
 */
function scrapedFixtureMatch(itemId: string): ScrapeStreamHit | null {
  if (itemId !== SYNTHETIC_MOVIE_ID) return null;
  return streamHitOf(
    {
      kind: "movie",
      id: "693134",
      title: "Dune: Part Two",
      year: 2024,
      director: "Denis Villeneuve",
      runtime: 166,
      overview:
        "Paul Atreides unites with the Fremen while seeking revenge against the conspirators.",
      genres: ["Science Fiction", "Adventure"],
      rating: 8.1,
      votes: 5400,
      cast: [["Timothée Chalamet", "Paul Atreides"]],
      imdb: "tt15239678",
      confidence: 0.98,
    },
    200,
    ["tmdb"],
  );
}

export interface SyntheticScrapeBackend extends ScrapeBackend {
  /** Every scrape applied so far, oldest first, for tests. */
  readonly applied: readonly { target: ScrapeTarget; request: ScrapeRequest }[];
  /** The tokens of the scrapes that were undone. */
  readonly undone: readonly string[];
  /** The item ids of every search-only call, oldest first, for tests. */
  readonly searched: readonly string[];
  /** The item ids of every Accept all confident call, oldest first, for tests. */
  readonly acceptedConfident: readonly (readonly string[])[];
}

/**
 * An in-memory backend: the streams of the fixtures above, and scrapes that are only recorded. A
 * title is matched by its words; an ID shortcut such as `tt0069293` finds one title exactly.
 */
export function createSyntheticScrapeBackend(
  options: {
    generic?: boolean;
    /** The title an item is linked to, as the backend's current-match lookup returns it. */
    current?: (itemId: string) => ScrapeStreamHit | null;
  } = {},
): SyntheticScrapeBackend {
  const applied: { target: ScrapeTarget; request: ScrapeRequest }[] = [];
  const undone: string[] = [];
  const searched: string[] = [];
  const acceptedConfident: (readonly string[])[] = [];
  return {
    available: true,
    applied,
    undone,
    searched,
    acceptedConfident,
    searchTransport: () =>
      perSearchTransport((request) =>
        createSyntheticScrapeStream(
          request.query,
          request.params?.kind === "show" ? "show" : "movie",
          options.generic === true,
        ),
      ),
    previewTransport: () => perSearchTransport(() => createSyntheticScrapePreview()),
    selection: (kind, preset, custom) => Promise.resolve(localFieldSelection(kind, preset, custom)),
    scrape: (target, request): Promise<ScrapeApplied> => {
      applied.push({ target, request });
      return Promise.resolve({ undo: `undo-${applied.length}`, historyEntry: null });
    },
    undo: (token) => {
      undone.push(token);
      return Promise.resolve();
    },
    searchOnly: (itemId, kind, query) => {
      searched.push(itemId);
      return Promise.resolve(syntheticSearchOnly(query, kind, options.generic === true));
    },
    currentMatch: (itemId) =>
      Promise.resolve(
        options.current?.(itemId) ??
          (options.generic === true ? scrapedFixtureMatch(itemId) : null),
      ),
    acceptConfident: (itemIds) => {
      acceptedConfident.push(itemIds);
      return Promise.resolve({
        accepted: itemIds.map((itemId, index) => ({
          itemId,
          undo: `confident-${acceptedConfident.length}-${index}`,
          reviewId: null,
        })),
        historyEntry: null,
        skipped: [],
        failed: [],
      });
    },
  };
}
