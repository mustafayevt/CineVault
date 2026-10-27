import type {
  ReviewBackend,
  ReviewCandidate,
  ReviewEvidenceTick,
  ReviewItem,
  ReviewLocalEvidence,
} from "./reviewBackend";

/** Fixtures for a plain browser and for tests: the Solaris scenario plus a longer queue. */

interface CandidateSpec {
  title: string;
  year: number;
  confidence: number;
  director?: string;
  runtimeMinutes?: number;
  language?: string;
  overview?: string;
}

function tick(
  kind: ReviewEvidenceTick["kind"],
  verdict: ReviewEvidenceTick["verdict"],
  text: string,
): ReviewEvidenceTick {
  return { kind, verdict, text };
}

function candidateOf(spec: CandidateSpec, local: ReviewLocalEvidence): ReviewCandidate {
  const runtime = spec.runtimeMinutes ?? null;
  const localMinutes = local.runtimeSeconds === null ? null : Math.round(local.runtimeSeconds / 60);
  const evidence: ReviewEvidenceTick[] = [tick("title", "agree", "title 1.00")];
  if (runtime !== null && localMinutes !== null) {
    const close = Math.abs(runtime - localMinutes) <= Math.max(1, localMinutes * 0.05);
    evidence.push(
      close
        ? tick("runtime", "agree", `runtime ${runtime} = ${localMinutes} min`)
        : tick("runtime", "conflict", `runtime ${runtime} ≠ ${localMinutes} min`),
    );
  }
  if (spec.language !== undefined && local.audioLanguage !== null) {
    evidence.push(
      spec.language === local.audioLanguage
        ? tick("language", "agree", `language ${spec.language} = ${local.audioLanguage}`)
        : tick("language", "conflict", `language ${spec.language} ≠ ${local.audioLanguage}`),
    );
  }
  if (local.year === null) evidence.push(tick("year", "unknown", "year not in filename"));
  else if (local.year === spec.year) evidence.push(tick("year", "agree", `year ${spec.year}`));
  else evidence.push(tick("year", "conflict", `year ${spec.year} ≠ ${local.year}`));
  const yearPenalty = local.year === null || local.year === spec.year ? 0 : 0.011;
  const evidenceDelta = evidence.reduce(
    (sum, entry) =>
      sum + (entry.verdict === "agree" ? 0.02 : entry.verdict === "conflict" ? -0.04 : 0),
    0,
  );
  return {
    provider: "tmdb",
    providerId: `${spec.title.toLowerCase().replaceAll(/\W+/g, "-")}-${spec.year}`,
    title: spec.title,
    originalTitle: null,
    year: spec.year,
    director: spec.director ?? null,
    runtimeMinutes: runtime,
    language: spec.language ?? null,
    overview: spec.overview ?? null,
    posterKey: null,
    confidence: spec.confidence / 100,
    rank: spec.confidence / 100,
    evidence,
    breakdown: { similarity: 1, yearPenalty, posterPenalty: 0, evidence: evidenceDelta },
  };
}

interface ItemSpec {
  id: number;
  name: string;
  path: string;
  title: string;
  year: number | null;
  reason: string;
  attention: boolean;
  note?: string;
  runtimeSeconds?: number;
  audioLanguage?: string;
  tech?: string;
  hasNfo?: boolean;
  ids?: string[];
  candidates: CandidateSpec[];
}

function itemOf(spec: ItemSpec): ReviewItem {
  const local: ReviewLocalEvidence = {
    title: spec.title,
    year: spec.year,
    runtimeSeconds: spec.runtimeSeconds ?? null,
    techSummary: spec.tech ?? null,
    audioLanguage: spec.audioLanguage ?? null,
    ids: spec.ids ?? [],
    hasNfo: spec.hasNfo ?? false,
    frameKey: null,
    frameAtSeconds:
      spec.runtimeSeconds === undefined ? null : Math.round(spec.runtimeSeconds / 4.054),
  };
  return {
    id: spec.id,
    itemId: `m-${spec.id}`,
    kind: "movie",
    name: spec.name,
    path: spec.path,
    reason: spec.reason,
    attention: spec.attention,
    note: spec.note ?? null,
    tied: spec.reason.includes("strong candidates"),
    shortPath: "",
    local,
    candidates: spec.candidates.map((candidate) => candidateOf(candidate, local)),
  };
}

const SPECS: ItemSpec[] = [
  {
    id: 1,
    name: "Solaris\\Solaris.mkv",
    path: "F:\\Personal\\Movie\\Solaris\\Solaris.mkv",
    title: "Solaris",
    year: null,
    reason: "Same title, 2 films",
    attention: true,
    note: "Two films share this exact title and the filename has no year. Runtime and audio language point strongly to one of them.",
    runtimeSeconds: 10012,
    audioLanguage: "ru",
    tech: "1080p · AVC · FLAC 1.0",
    candidates: [
      {
        title: "Solaris",
        year: 1972,
        confidence: 91,
        director: "Andrei Tarkovsky",
        runtimeMinutes: 167,
        language: "ru",
        overview:
          "A psychologist is sent to a space station orbiting an ocean planet that seems to answer the crew’s memories.",
      },
      {
        title: "Solaris",
        year: 2002,
        confidence: 58,
        director: "Steven Soderbergh",
        runtimeMinutes: 99,
        language: "en",
        overview:
          "A grieving psychologist travels to a station above Solaris and meets someone who should not exist.",
      },
    ],
  },
  {
    id: 2,
    name: "the.batman.2022.2160p.uhd.remux",
    path: "F:\\Personal\\Movie\\the.batman.2022.2160p.uhd.remux\\the.batman.2022.2160p.uhd.remux.mkv",
    title: "the batman",
    year: 2022,
    reason: "No NFO",
    attention: false,
    runtimeSeconds: 10560,
    audioLanguage: "en",
    tech: "2160p · HEVC · TrueHD 7.1",
    candidates: [
      {
        title: "The Batman",
        year: 2022,
        confidence: 94,
        director: "Matt Reeves",
        runtimeMinutes: 176,
        language: "en",
      },
      { title: "Batman", year: 1989, confidence: 52, director: "Tim Burton", runtimeMinutes: 126 },
    ],
  },
  {
    id: 3,
    name: "Zodiac.2007.1080p.BluRay",
    path: "F:\\Personal\\Movie\\Zodiac.2007.1080p.BluRay\\Zodiac.2007.1080p.BluRay.mkv",
    title: "Zodiac",
    year: 2007,
    reason: "NFO year differs",
    attention: true,
    runtimeSeconds: 9420,
    audioLanguage: "en",
    tech: "1080p · AVC · DTS 5.1",
    hasNfo: true,
    candidates: [
      {
        title: "Zodiac",
        year: 2007,
        confidence: 88,
        director: "David Fincher",
        runtimeMinutes: 157,
        language: "en",
      },
      { title: "Zodiac", year: 1996, confidence: 49, runtimeMinutes: 94, language: "en" },
    ],
  },
  {
    id: 4,
    name: "Heat.1995.Directors.Definitive.Edition",
    path: "F:\\Personal\\Movie\\Heat.1995.Directors.Definitive.Edition\\Heat.mkv",
    title: "Heat",
    year: 1995,
    reason: "Edition detected",
    attention: false,
    runtimeSeconds: 10200,
    audioLanguage: "en",
    tech: "1080p · AVC · DTS-HD MA 5.1",
    candidates: [
      {
        title: "Heat",
        year: 1995,
        confidence: 97,
        director: "Michael Mann",
        runtimeMinutes: 170,
        language: "en",
      },
    ],
  },
  {
    id: 5,
    name: "Crash (2004)",
    path: "F:\\Personal\\Movie\\Crash (2004)\\Crash.mkv",
    title: "Crash",
    year: 2004,
    reason: "3 candidates",
    attention: true,
    runtimeSeconds: 6600,
    audioLanguage: "en",
    tech: "720p · AVC · AC-3 5.1",
    candidates: [
      {
        title: "Crash",
        year: 2004,
        confidence: 64,
        director: "Paul Haggis",
        runtimeMinutes: 112,
        language: "en",
      },
      {
        title: "Crash",
        year: 1996,
        confidence: 60,
        director: "David Cronenberg",
        runtimeMinutes: 100,
        language: "en",
      },
      { title: "Crash", year: 2005, confidence: 41, runtimeMinutes: 94, language: "en" },
    ],
  },
  {
    id: 6,
    name: "Godzilla",
    path: "F:\\Personal\\Movie\\Godzilla\\Godzilla.mkv",
    title: "Godzilla",
    year: null,
    reason: "6 candidates · no year",
    attention: true,
    runtimeSeconds: 7140,
    audioLanguage: "en",
    tech: "1080p · AVC · AC-3 5.1",
    candidates: [
      { title: "Godzilla", year: 2014, confidence: 41, runtimeMinutes: 123, language: "en" },
      { title: "Godzilla", year: 1998, confidence: 40, runtimeMinutes: 139, language: "en" },
      { title: "Godzilla", year: 1954, confidence: 38, runtimeMinutes: 96, language: "ja" },
    ],
  },
  {
    id: 7,
    name: "Les.Miserables.2012.REPACK",
    path: "F:\\Personal\\Movie\\Les.Miserables.2012.REPACK\\Les.Miserables.2012.REPACK.mkv",
    title: "Les Miserables",
    year: 2012,
    reason: "Low title score",
    attention: true,
    runtimeSeconds: 9480,
    audioLanguage: "en",
    tech: "1080p · AVC · DTS 5.1",
    candidates: [
      {
        title: "Les Misérables",
        year: 2012,
        confidence: 72,
        director: "Tom Hooper",
        runtimeMinutes: 158,
        language: "en",
      },
      { title: "Les Misérables", year: 1998, confidence: 44, runtimeMinutes: 134, language: "en" },
    ],
  },
  {
    id: 8,
    name: "Dune\\dune.mkv",
    path: "F:\\Personal\\Movie\\Dune\\dune.mkv",
    title: "Dune",
    year: null,
    reason: "1984 vs 2021",
    attention: true,
    runtimeSeconds: 9300,
    audioLanguage: "en",
    tech: "1080p · HEVC · EAC3 5.1",
    candidates: [
      { title: "Dune", year: 2021, confidence: 55, runtimeMinutes: 155, language: "en" },
      { title: "Dune", year: 1984, confidence: 54, runtimeMinutes: 137, language: "en" },
    ],
  },
  {
    id: 9,
    name: "Blade.Runner.2049.2017.1080p",
    path: "F:\\Personal\\Movie\\Blade.Runner.2049.2017.1080p\\Blade.Runner.2049.2017.1080p.mkv",
    title: "Blade Runner 2049",
    year: 2017,
    reason: "No NFO",
    attention: false,
    runtimeSeconds: 9840,
    audioLanguage: "en",
    candidates: [
      {
        title: "Blade Runner 2049",
        year: 2017,
        confidence: 92,
        director: "Denis Villeneuve",
        runtimeMinutes: 164,
        language: "en",
      },
    ],
  },
  {
    id: 10,
    name: "Alien.1979.Final.Cut",
    path: "F:\\Personal\\Movie\\Alien.1979.Final.Cut\\Alien.mkv",
    title: "Alien",
    year: 1979,
    reason: "Edition detected",
    attention: false,
    runtimeSeconds: 7020,
    audioLanguage: "en",
    candidates: [
      {
        title: "Alien",
        year: 1979,
        confidence: 89,
        director: "Ridley Scott",
        runtimeMinutes: 117,
        language: "en",
      },
    ],
  },
  {
    id: 11,
    name: "The.Thing.1982.720p",
    path: "F:\\Personal\\Movie\\The.Thing.1982.720p\\The.Thing.1982.720p.mkv",
    title: "The Thing",
    year: 1982,
    reason: "No NFO",
    attention: false,
    runtimeSeconds: 6540,
    audioLanguage: "en",
    candidates: [
      {
        title: "The Thing",
        year: 1982,
        confidence: 86,
        director: "John Carpenter",
        runtimeMinutes: 109,
        language: "en",
      },
    ],
  },
  {
    id: 12,
    name: "Parasite.2019.KOREAN",
    path: "F:\\Personal\\Movie\\Parasite.2019.KOREAN\\Parasite.2019.KOREAN.mkv",
    title: "Parasite",
    year: 2019,
    reason: "Language tag in name",
    attention: false,
    runtimeSeconds: 7920,
    audioLanguage: "ko",
    candidates: [
      {
        title: "Parasite",
        year: 2019,
        confidence: 95,
        director: "Bong Joon-ho",
        runtimeMinutes: 132,
        language: "ko",
      },
    ],
  },
  {
    id: 13,
    name: "Arrival.2016.2160p",
    path: "F:\\Personal\\Movie\\Arrival.2016.2160p\\Arrival.2016.2160p.mkv",
    title: "Arrival",
    year: 2016,
    reason: "No NFO",
    attention: false,
    runtimeSeconds: 6960,
    audioLanguage: "en",
    candidates: [
      {
        title: "Arrival",
        year: 2016,
        confidence: 90,
        director: "Denis Villeneuve",
        runtimeMinutes: 116,
        language: "en",
      },
    ],
  },
  {
    id: 14,
    name: "VID_20190812_birthday",
    path: "F:\\Personal\\Movie\\VID_20190812_birthday\\VID_20190812_birthday.mp4",
    title: "VID 20190812 birthday",
    year: null,
    reason: "No candidates found",
    attention: true,
    note: "This looks like a home video. Ignore it and it will not be queued again.",
    runtimeSeconds: 312,
    tech: "1080p · AVC · AAC 2.0",
    candidates: [],
  },
];

/** The fixture queue: 14 items, the first being the Solaris scenario. */
export function syntheticReviewItems(): ReviewItem[] {
  return SPECS.map(itemOf);
}

function idShortcut(query: string): string | null {
  const imdb = /\btt(\d{7,9})\b/i.exec(query);
  if (imdb !== null) return imdb[0].toLowerCase();
  const prefixed = /\b(?:tmdb|tvdb|imdb):\s*([\w-]+)/i.exec(query);
  return prefixed?.[1]?.toLowerCase() ?? null;
}

interface SyntheticReviewOptions {
  /**
   * How many queue items to list; read each time the queue is read. A plain browser lists as many
   * as the pipeline counts say, so the sidebar and the page show one number.
   */
  limit?: () => Promise<number>;
}

/**
 * An in-memory backend. Ignored items stay ignored for as long as the backend lives, like the
 * database row of the desktop app, and `reopen` brings accepted or ignored items back.
 */
export function createSyntheticReviewBackend(
  items: readonly ReviewItem[] = syntheticReviewItems(),
  options: SyntheticReviewOptions = {},
): ReviewBackend {
  const all = [...items];
  const closed = new Set<number>();
  const pending = () => all.filter((item) => !closed.has(item.id));
  const known = () => all.flatMap((item) => item.candidates);
  return {
    available: true,
    list: async () => {
      const limit = options.limit === undefined ? undefined : await options.limit();
      return limit === undefined ? pending() : pending().slice(0, limit);
    },
    count: () => Promise.resolve(pending().length),
    accept: (reviewId) => {
      closed.add(reviewId);
      return Promise.resolve();
    },
    acceptTop: (reviewIds) => {
      const accepted = pending()
        .filter((item) => reviewIds.includes(item.id) && item.candidates.length > 0)
        .map((item) => item.id);
      accepted.forEach((id) => closed.add(id));
      return Promise.resolve(accepted);
    },
    ignore: (reviewId) => {
      closed.add(reviewId);
      return Promise.resolve();
    },
    reopen: (reviewIds) => {
      reviewIds.forEach((id) => closed.delete(id));
      return Promise.resolve();
    },
    search: (_reviewId, query) => {
      const shortcut = idShortcut(query);
      const needle = query.trim().toLowerCase();
      const hits =
        shortcut === "tt0069293"
          ? known().filter((candidate) => candidate.title === "Solaris" && candidate.year === 1972)
          : known().filter((candidate) => candidate.title.toLowerCase().includes(needle));
      const unique = new Map(hits.map((hit) => [hit.providerId, hit]));
      return Promise.resolve(
        [...unique.values()].sort((a, b) => b.confidence - a.confidence).slice(0, 5),
      );
    },
    matchItems: (itemIds) => Promise.resolve(itemIds.length),
    startBatch: (_mode, itemIds) =>
      Promise.resolve({ planned: itemIds === null ? 0 : itemIds.length, withoutProvider: 0 }),
  };
}
