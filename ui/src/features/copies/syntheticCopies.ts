import type { Comparison, CopiesBackend, CopiesOf, CopyRow } from "./copiesBackend";

/**
 * Two pairs of copies in the fixture library, for a plain browser, tests and the screenshots: the
 * movies at these indexes of the generated list take the facts below instead of the generated
 * ones. One film is a 2160p Dolby Vision disc rip next to a 1080p download; the other a 2160p
 * HDR10 rip next to a 1080p one.
 */
export interface CopyFixture {
  /** Index of the copy in the generated list; its id is the index plus one. */
  index: number;
  group: number;
  title: string;
  year: number;
  resolution: "2160p" | "1080p";
  hdrCard: "DV" | "HDR10" | null;
  hdrFormats: string[];
  width: number;
  height: number;
  bitrateKbps: number;
  sizeBytes: number;
  audio: string;
  audioCodec: string;
  audioChannels: number;
  source: string;
  folder: string;
  file: string;
  sourceLabel: string;
  durationMinutes: number;
}

const FIXTURES: readonly CopyFixture[] = [
  {
    index: 4,
    group: 1,
    title: "Example Movie",
    year: 2020,
    resolution: "2160p",
    hdrCard: "DV",
    hdrFormats: ["Dolby Vision", "HDR10"],
    width: 3840,
    height: 2160,
    bitrateKbps: 62_800,
    sizeBytes: 58_300_000_000,
    audio: "TrueHD Atmos 7.1",
    audioCodec: "TrueHD Atmos",
    audioChannels: 8,
    source: "BluRay",
    folder: "D:\\Movies\\Example Movie (2020)",
    file: "Example.Movie.2020.2160p.BluRay.REMUX.DV.mkv",
    sourceLabel: "Media (D:)",
    durationMinutes: 112,
  },
  {
    index: 5,
    group: 1,
    title: "Example Movie",
    year: 2020,
    resolution: "1080p",
    hdrCard: null,
    hdrFormats: [],
    width: 1920,
    height: 1080,
    bitrateKbps: 6_600,
    sizeBytes: 6_100_000_000,
    audio: "AAC 5.1",
    audioCodec: "AAC",
    audioChannels: 6,
    source: "WEB-DL",
    folder: "\\\\nas\\movies\\Films\\Example.Movie.2020.1080p.WEB-DL.AAC5.1.H264",
    file: "Example.Movie.2020.1080p.WEB-DL.AAC5.1.H264.mkv",
    sourceLabel: "nas",
    durationMinutes: 112,
  },
  {
    index: 30,
    group: 2,
    title: "Another Film",
    year: 2018,
    resolution: "2160p",
    hdrCard: "HDR10",
    hdrFormats: ["HDR10"],
    width: 3840,
    height: 2160,
    bitrateKbps: 58_000,
    sizeBytes: 98_400_000_000,
    audio: "DTS-HD MA 7.1",
    audioCodec: "DTS-HD MA",
    audioChannels: 8,
    source: "BluRay",
    folder: "D:\\Movies\\Another Film (2018)",
    file: "Another.Film.2018.2160p.BluRay.HDR10.mkv",
    sourceLabel: "Media (D:)",
    durationMinutes: 134,
  },
  {
    index: 31,
    group: 2,
    title: "Another Film",
    year: 2018,
    resolution: "1080p",
    hdrCard: null,
    hdrFormats: [],
    width: 1920,
    height: 1080,
    bitrateKbps: 14_600,
    sizeBytes: 19_800_000_000,
    audio: "DTS-HD MA 5.1",
    audioCodec: "DTS-HD MA",
    audioChannels: 6,
    source: "BluRay",
    folder: "\\\\nas\\movies\\Films\\Another Film (2018)",
    file: "Another.Film.2018.1080p.BluRay.DTS-HD.MA.5.1.x264.mkv",
    sourceLabel: "nas",
    durationMinutes: 134,
  },
];

const BY_INDEX = new Map(FIXTURES.map((fixture) => [fixture.index, fixture]));
const BY_ID = new Map(FIXTURES.map((fixture) => [String(fixture.index + 1), fixture]));

/** The copy fixture at a position of the generated list, when it is one. */
export function copyFixtureAt(index: number): CopyFixture | undefined {
  return BY_INDEX.get(index);
}

/** The ids of every copy in the group of `id`, in id order; empty when `id` has none. */
function groupIds(id: string): string[] {
  const group = BY_ID.get(id)?.group;
  return FIXTURES.filter((fixture) => fixture.group === group).map((f) => String(f.index + 1));
}

/** How many copies the fixture group of a position has (0 when it is not a copy). */
export function copyGroupSizeAt(index: number): number {
  return index < 0 ? 0 : groupIds(String(index + 1)).length;
}

const kept = new Set<number>();
let version = 0;

/** Changes whenever a decision is made, so cached listings are rebuilt. */
export function copiesVersion(): number {
  return version;
}

/** Whether the user kept the group of the fixture at `index`. */
export function copyGroupKeptAt(index: number): boolean {
  const group = BY_INDEX.get(index)?.group;
  return group !== undefined && kept.has(group);
}

/** The rank of a copy inside its group, best first (0). */
export function copyRankAt(index: number): number {
  const fixture = BY_INDEX.get(index);
  if (fixture === undefined) return 0;
  return FIXTURES.filter((f) => f.group === fixture.group).findIndex((f) => f.index === index);
}

/** The group a fixture belongs to; 0 when it is none. */
export function copyGroupAt(index: number): number {
  return BY_INDEX.get(index)?.group ?? 0;
}

/** Forgets every decision; tests and the screenshots start from here. */
export function resetSyntheticCopies(): void {
  kept.clear();
  version += 1;
}

function rowOf(fixture: CopyFixture, best: boolean): CopyRow {
  return {
    id: String(fixture.index + 1),
    title: fixture.title,
    year: fixture.year,
    media: {
      resolution: fixture.resolution,
      hdr_formats: fixture.hdrFormats,
      audio: fixture.audio,
      video_bitrate_kbps: fixture.bitrateKbps,
      duration_minutes: fixture.durationMinutes,
    },
    source: fixture.source,
    size_bytes: fixture.sizeBytes,
    folder: fixture.folder,
    file_name: fixture.file,
    source_label: fixture.sourceLabel,
    best,
  };
}

/** The copies backend over the fixtures: the decisions live in memory. */
export function createSyntheticCopies(): CopiesBackend {
  return {
    copiesOf: (id): Promise<CopiesOf | null> => {
      const ids = groupIds(id);
      const group = BY_ID.get(id)?.group;
      return Promise.resolve(
        ids.length < 2 || group === undefined ? null : { ids, kept: kept.has(group) },
      );
    },
    compare: (id): Promise<Comparison | null> => {
      const own = BY_ID.get(id);
      if (own === undefined) return Promise.resolve(null);
      const members = FIXTURES.filter((f) => f.group === own.group);
      const ordered = [own, ...members.filter((f) => f !== own)];
      return Promise.resolve({
        copies: ordered.map((fixture) => rowOf(fixture, fixture === members[0])),
        kept: kept.has(own.group),
        look_same: false,
      });
    },
    keepAll: (ids) => {
      const group = ids.map((id) => BY_ID.get(id)?.group).find((g) => g !== undefined);
      if (group !== undefined) kept.add(group);
      version += 1;
      return Promise.resolve();
    },
    askAgain: (id) => {
      const group = BY_ID.get(id)?.group;
      if (group !== undefined) kept.delete(group);
      version += 1;
      return Promise.resolve();
    },
    labelToDrop: () => Promise.resolve(null),
  };
}

/** The number of movies in open groups among the first `total` fixture movies. */
export function openCopiesAmong(total: number): number {
  return FIXTURES.filter((f) => f.index < total && !kept.has(f.group)).length;
}
