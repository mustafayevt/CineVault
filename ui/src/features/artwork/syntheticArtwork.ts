import {
  createFakeProviderTransport,
  perSearchTransport,
} from "../workspaces/fakeProviderTransport";
import type {
  FakeProviderScript,
  FakeProviderTransport,
} from "../workspaces/fakeProviderTransport";
import { ArtworkRequestError, CURRENT_PROVIDER } from "./artworkBackend";
import type { ArtworkImage, ArtworkSlotCount } from "./artworkBackend";
import type {
  ArtworkBackend,
  ArtworkCandidate,
  ArtworkPlaced,
  ArtworkRun,
  ArtworkStreamStart,
  LocalImage,
} from "./artworkBackend";

/**
 * Fixtures for a plain browser and for tests: a title with candidates from three providers in
 * several languages and sizes. The ThumbHashes are those of the detail fixture, so the cards show
 * a preview where no image can be loaded.
 */

const POSTER_HASH = "8ce705050088887f78878787877887877680770778";
const BACKDROP_HASH = "d1690a2d0888887f7787778777788887887f87f887";
const LOGO_HASH = "4db6091d0688887f77877788777888877780770778";

const hex = (n: number): string => n.toString(16).padStart(32, "0");

const PROVIDERS = [
  { key: "tmdb", name: "TMDB" },
  { key: "fanart", name: "Fanart.tv" },
  { key: "tvdb", name: "TheTVDB" },
] as const;

/** Every slot a movie can have, then the two kinds of extras. */
const SYNTHETIC_SLOTS: readonly string[] = [
  "poster",
  "fanart",
  "clearlogo",
  "thumb",
  "keyart",
  "banner",
  "clearart",
  "disc",
  "extrafanart",
  "extrathumb",
];

type Row = readonly [
  provider: string,
  language: string | null,
  width: number,
  height: number,
  score: number,
];

const POSTERS: readonly Row[] = [
  ["tmdb", "en", 2000, 3000, 90],
  ["tmdb", "en", 1000, 1500, 90],
  ["fanart", "en", 1000, 1426, 90],
  ["tmdb", "tr", 2000, 3000, 50],
  ["tvdb", "en", 680, 1000, 50],
  ["tmdb", "ja", 2000, 3000, 0],
  ["fanart", "fr", 1000, 1426, 0],
  ["tmdb", null, 2000, 3000, 40],
  ["tvdb", "de", 680, 1000, 0],
  ["tmdb", "en", 500, 750, 50],
];

const WIDE: readonly Row[] = [
  ["tmdb", null, 3840, 2160, 90],
  ["fanart", null, 1920, 1080, 90],
  ["tmdb", "en", 3840, 2160, 90],
  ["tvdb", null, 1920, 1080, 40],
  ["tmdb", null, 1280, 720, 40],
  ["fanart", "en", 1920, 1080, 50],
  ["tmdb", "ja", 3840, 2160, 0],
  ["tvdb", null, 1280, 720, 40],
  ["fanart", null, 3840, 2160, 90],
  ["tmdb", null, 1920, 1080, 40],
  ["tvdb", null, 1920, 1080, 40],
  ["tmdb", null, 2560, 1440, 40],
];

const LOGOS: readonly Row[] = [
  ["fanart", "en", 800, 310, 90],
  ["tmdb", "en", 500, 194, 90],
  ["fanart", null, 800, 310, 40],
  ["tmdb", "ja", 500, 194, 0],
  // Too small for a logo: listed behind "Show 2 more".
  ["fanart", "en", 240, 93, 90],
  ["tmdb", null, 180, 70, 40],
];

const SQUARE: readonly Row[] = [
  ["fanart", null, 1000, 1000, 40],
  ["fanart", "en", 1000, 1000, 90],
];

const BANNERS: readonly Row[] = [
  ["fanart", "en", 1000, 185, 90],
  ["tvdb", "en", 758, 140, 50],
];

function rowsOf(slot: string): readonly Row[] {
  switch (slot) {
    case "poster":
    case "keyart":
      return POSTERS;
    case "clearlogo":
    case "clearart":
      return LOGOS;
    case "disc":
      return SQUARE;
    case "banner":
      return BANNERS;
    default:
      return WIDE;
  }
}

function hashOf(slot: string): string {
  if (slot === "poster" || slot === "keyart") return POSTER_HASH;
  if (slot === "clearlogo" || slot === "clearart" || slot === "disc") return LOGO_HASH;
  return BACKDROP_HASH;
}

/** The width under which an image of a slot is too small, as the backend judges it. */
function minimumWidth(slot: string): number {
  if (slot === "poster" || slot === "keyart") return 500;
  if (slot === "fanart" || slot === "extrafanart") return 1280;
  return 300;
}

/** Whether an image has the wrong shape for its slot, as the backend judges it. */
function misshapen(slot: string, width: number, height: number): boolean {
  const ratio = width / height;
  if (slot === "poster" || slot === "keyart") return ratio < 0.55 || ratio > 0.8;
  if (slot === "fanart" || slot === "thumb") return ratio < 1.4 || ratio > 2.1;
  return false;
}

/** Votes on TMDB images and likes on Fanart.tv ones, falling with the place in the list. */
const VOTES: readonly number[] = [48, 22, 0, 9, 3, 1, 0, 14, 0, 2, 0, 5, 0];
const LIKES: readonly number[] = [31, 12, 7, 2, 0, 4, 0, 1, 18, 0, 0, 0, 0];

/** The candidates of a slot. Their ids are web addresses, as the real ones are. */
function syntheticCandidates(slot: string): ArtworkCandidate[] {
  const base = SYNTHETIC_SLOTS.indexOf(slot) + 1;
  return rowsOf(slot).map(([provider, language, width, height, score], index) => {
    const votes = provider === "tmdb" ? (VOTES[index] ?? 0) : 0;
    const likes = provider === "fanart" ? (LIKES[index] ?? 0) : 0;
    const rating = votes === 0 ? 0 : 52 + ((index * 7) % 12);
    // Like the backend's key: the fit first, then how much people like it and its size.
    const liked = Math.min(9, Math.log1p(likes) * 2 + Math.log1p(votes) * (rating / 60));
    return {
      id: `https://img.example/${provider}/${slot}/${index + 1}.jpg`,
      provider,
      language,
      width,
      height,
      score,
      best: Math.round((score * 1000 + liked * 10 + Math.min(width / 400, 9)) * 100),
      votes,
      rating,
      likes,
      low_quality: width < minimumWidth(slot) || misshapen(slot, width, height),
      image: { key: hex(base * 100 + index + 1), thumbhash: hashOf(slot), ambient: null },
    };
  });
}

/** Eight evenly spaced stills from the synthetic title's video, offered as the provider "frames". */
function syntheticFrames(slot: string): ArtworkCandidate[] {
  return Array.from({ length: 8 }, (_, index) => ({
    id: `file:///cache/frames/1/frame-${String(index + 1).padStart(2, "0")}.jpg`,
    provider: "frames",
    language: null,
    width: 1920,
    height: 1080,
    score: 30,
    best: 30_000_00 - index,
    votes: 0,
    rating: 0,
    likes: 0,
    low_quality: false,
    image: { key: hex(8000 + index), thumbhash: hashOf(slot), ambient: null },
  }));
}

/** The image the synthetic title has now for a slot, which a stream pins first. */
export function syntheticCurrentArtwork(slot: string): ArtworkCandidate {
  return {
    id: `file:///library/Dune%20Part%20Two%20(2024)/${slot}.jpg`,
    provider: CURRENT_PROVIDER,
    language: null,
    width: slot === "poster" ? 1000 : 1920,
    height: slot === "poster" ? 1500 : 1080,
    score: 0,
    best: 0,
    votes: 0,
    rating: 0,
    likes: 0,
    low_quality: false,
    image: {
      key: hex(9000 + SYNTHETIC_SLOTS.indexOf(slot)),
      thumbhash: hashOf(slot),
      ambient: null,
    },
  };
}

/**
 * The synthetic backend's artwork stream: Fanart.tv answers first, TheTVDB fails once (and
 * delivers when asked again) and TMDB is the slow one. Providers that do not offer the slot are
 * left out, as the real search leaves them out.
 */
export function createSyntheticArtworkStream(
  slot: string,
): FakeProviderTransport<ArtworkCandidate> {
  const all = syntheticCandidates(slot);
  const of = (provider: string) => all.filter((candidate) => candidate.provider === provider);
  const offered = PROVIDERS.filter((provider) => of(provider.key).length > 0);
  const script: Record<string, FakeProviderScript<ArtworkCandidate>> = {};
  for (const { key } of offered) {
    if (key === "fanart") script[key] = { delayMs: 100, items: of(key) };
    else if (key === "tvdb") {
      script[key] = {
        delayMs: 250,
        fail: { reason: "TheTVDB request failed: timed out", retryable: true },
        onRetry: { delayMs: 100, items: of(key) },
      };
    } else script[key] = { delayMs: 400, items: of(key), batchSize: 3, batchMs: 100 };
  }
  // Frames from the video: eight stills for thumbs and fanart, made last.
  if (slot === "fanart" || slot === "thumb" || slot === "extrafanart" || slot === "extrathumb") {
    script["frames"] = { delayMs: 600, items: syntheticFrames(slot) };
  }
  return createFakeProviderTransport<ArtworkCandidate>(script);
}

/** The seasons of the synthetic show (`syntheticTv`), whose picker offers the season types. */
const SYNTHETIC_SEASONS: Readonly<Record<string, string>> = {
  "11": "Severance · Season 1",
  "12": "Severance · Season 2",
};

/** The types a season can have, by their plain names. */
const SYNTHETIC_SEASON_SLOTS: readonly string[] = ["poster", "fanart", "banner", "thumb"];

/** What the synthetic search tells at once: the title, the slots, the providers and the current image. */
function syntheticStart(
  slot: string,
  options: { title?: string; year?: number | null },
  itemId = "",
): ArtworkStreamStart {
  const season = SYNTHETIC_SEASONS[itemId];
  if (season !== undefined) {
    return {
      ...syntheticStart(slot, options),
      title: season,
      year: 2022,
      slots: [...SYNTHETIC_SEASON_SLOTS],
    };
  }
  const offered = PROVIDERS.filter((provider) =>
    syntheticCandidates(slot).some((candidate) => candidate.provider === provider.key),
  );
  const frames = ["fanart", "thumb", "extrafanart", "extrathumb"].includes(slot)
    ? [{ key: "frames", name: "Frames" }]
    : [];
  const current = syntheticCurrentArtwork(slot);
  return {
    title: options.title ?? "Dune Part Two",
    year: options.year === undefined ? 2024 : options.year,
    slots: [...SYNTHETIC_SLOTS],
    providers: [...offered.map((provider) => ({ ...provider })), ...frames],
    current: {
      ...current,
      // The wire type says a score of 0 to 90; the current image has none.
      score: 0,
    },
  };
}

export interface SyntheticArtworkBackend extends ArtworkBackend {
  /** Everything placed so far, oldest first, for tests. */
  readonly placed: readonly { itemId: string; slot: string; sources: string[] }[];
  /** The tokens of the placements that were undone. */
  readonly undone: readonly string[];
  /** The candidates whose originals were asked for, oldest first. */
  readonly fullRequests: readonly string[];
}

function nameOf(image: LocalImage): string {
  return image.kind === "path" ? image.path : image.file.name;
}

export function createSyntheticArtworkBackend(
  options: {
    title?: string;
    year?: number | null;
    /** The counts of the types, as the backend's counts-only lookup returns them. */
    counts?: ArtworkSlotCount[];
    /** Answers for the original of a candidate; by default a key of its own, at once. */
    fullImage?: (candidateId: string) => Promise<ArtworkImage>;
  } = {},
): SyntheticArtworkBackend {
  const placed: { itemId: string; slot: string; sources: string[] }[] = [];
  const undone: string[] = [];
  const fullRequests: string[] = [];
  const record = (itemId: string, slot: string, sources: string[]): ArtworkPlaced => {
    placed.push({ itemId, slot, sources });
    return { images: sources.length, problems: [], undo: `place-${placed.length}` };
  };
  return {
    available: true,
    placed,
    undone,
    fullRequests,
    searchTransport: (onStart) => {
      const inner = perSearchTransport((request) =>
        createSyntheticArtworkStream(String(request.params?.slot ?? "poster")),
      );
      return {
        ...inner,
        start: (request, onEvent) => {
          const slot = String(request.params?.slot ?? "poster");
          onStart?.(syntheticStart(slot, options, String(request.params?.itemId ?? "")));
          inner.start(request, onEvent);
        },
      };
    },
    place: (itemId, slot, source) => {
      if (source.trim() === "") {
        return Promise.reject(new ArtworkRequestError("invalid", "There is no address to use."));
      }
      return Promise.resolve(record(itemId, slot, [source]));
    },
    placeLocal: (itemId, slot, image) => Promise.resolve(record(itemId, slot, [nameOf(image)])),
    placeExtras: (itemId, slot, sources) => Promise.resolve(record(itemId, slot, [...sources])),
    undoPlace: (token) => {
      undone.push(token);
      return Promise.resolve({ images: 1, problems: [], undo: null });
    },
    counts: () => Promise.resolve(options.counts ?? null),
    fullImage: (candidateId) => {
      fullRequests.push(candidateId);
      if (options.fullImage !== undefined) return options.fullImage(candidateId);
      const sum = Array.from(candidateId).reduce((all, char) => all + char.charCodeAt(0), 0);
      return Promise.resolve({ key: hex(20_000 + sum), thumbhash: null, ambient: null });
    },
    downloadMissing: (itemIds): Promise<ArtworkRun> =>
      Promise.resolve({ images: itemIds.length * 2, portraits: 0, skipped: 0 }),
    fillSeasonArtwork: (): Promise<ArtworkRun> =>
      Promise.resolve({ images: 0, portraits: 0, skipped: 0 }),
  };
}
