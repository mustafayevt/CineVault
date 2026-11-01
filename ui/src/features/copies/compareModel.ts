import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import type { Comparison, CopyRow } from "./copiesBackend";

/** The rows of the Compare copies page, top to bottom. */
export const ROW_IDS = [
  "resolution",
  "hdr",
  "audio",
  "source",
  "bitrate",
  "length",
  "size",
  "folder",
] as const;
export type RowId = (typeof ROW_IDS)[number];

/** How a value reads against the other copies' in its row. */
export type Verdict = "win" | "lose" | "same" | "plain";

/** One value of a card. */
export interface Cell {
  row: RowId;
  text: string;
  verdict: Verdict;
  /** A quiet note after the value: "52.2 GB smaller". */
  note?: string;
  /** The whole text when `text` is cut for the width (the folder path). */
  full?: string;
  mono?: boolean;
  /** The value tells against the copies being one film (lengths far apart). */
  warn?: boolean;
}

export interface Card {
  copy: CopyRow;
  /** "2160p · Dolby Vision", or "1080p". */
  heading: string;
  /** The label of the library folder, "Media (D:)". */
  drive: string;
  /** The copy lives on a network share. */
  network: boolean;
  cells: Cell[];
}

const RESOLUTION_RANK: Readonly<Record<string, number>> = {
  "4320p": 6,
  "2160p": 5,
  "1440p": 4,
  "1080p": 3,
  "720p": 2,
};

const RESOLUTION_NAME: Readonly<Record<string, MessageKey>> = {
  "2160p": "copies.res.2160p",
  "1440p": "copies.res.1440p",
  "1080p": "copies.res.1080p",
  "720p": "copies.res.720p",
};

const HDR_RANK: readonly (readonly [string, number])[] = [
  ["dolby vision", 4],
  ["dv", 4],
  ["hdr10+", 3],
  ["hdr10", 2],
  ["hlg", 1],
];

/** Best codec first, after the channels (the order the best-quality mark uses). */
const AUDIO_CODECS = [
  "truehd atmos",
  "dts-x",
  "truehd",
  "dts-hd ma",
  "e-ac-3 atmos",
  "dts-hd",
  "e-ac-3",
  "dts",
  "ac3",
  "aac",
];

const SOURCE_RANK: readonly (readonly [string, number])[] = [
  ["remux", 5],
  ["bluray", 5],
  ["blu-ray", 5],
  ["web-dl", 4],
  ["webrip", 3],
  ["hdtv", 2],
  ["dvd", 1],
];

function resolutionRank(copy: CopyRow): number | null {
  if (copy.media === null) return null;
  return RESOLUTION_RANK[copy.media.resolution] ?? 1;
}

function hdrRank(copy: CopyRow): number | null {
  if (copy.media === null) return null;
  let best = 0;
  for (const format of copy.media.hdr_formats) {
    const key = format.toLowerCase();
    for (const [name, rank] of HDR_RANK) if (key.includes(name)) best = Math.max(best, rank);
  }
  return best;
}

function audioRank(copy: CopyRow): number | null {
  if (copy.media === null || copy.media.audio === "") return null;
  const text = copy.media.audio.toLowerCase();
  const layout = /(\d+)\.(\d+)/.exec(text);
  const channels = layout === null ? 0 : Number(layout[1]) + Number(layout[2]);
  const codec = AUDIO_CODECS.findIndex((name) => text.startsWith(name));
  return channels * 20 + (codec < 0 ? 0 : AUDIO_CODECS.length - codec);
}

function sourceRank(copy: CopyRow): number | null {
  const key = copy.source.toLowerCase();
  if (key === "") return null;
  for (const [name, rank] of SOURCE_RANK) if (key.includes(name)) return rank;
  return 0;
}

function bitrateRank(copy: CopyRow): number | null {
  return copy.media === null || copy.media.video_bitrate_kbps === 0
    ? null
    : copy.media.video_bitrate_kbps;
}

/**
 * Which values win in a row: those with the highest rank when it is above another copy's. A row
 * where a copy has no rank (not read yet) names no winner; one where every rank is equal is "same".
 */
export function verdicts(ranks: readonly (number | null)[]): Verdict[] {
  const known = ranks.filter((rank): rank is number => rank !== null);
  if (known.length < 2 || known.length < ranks.length) return ranks.map(() => "plain");
  const top = Math.max(...known);
  if (top === Math.min(...known)) return ranks.map(() => "same");
  return ranks.map((rank) => (rank === top ? "win" : "lose"));
}

function resolutionText(copy: CopyRow): string {
  const media = copy.media;
  if (media === null) return t("copies.value.unread");
  if (media.resolution === "") return t("copies.value.unknown");
  const key = RESOLUTION_NAME[media.resolution];
  return key === undefined ? media.resolution : `${media.resolution} · ${t(key)}`;
}

function hdrText(copy: CopyRow): string {
  if (copy.media === null) return t("copies.value.unread");
  return copy.media.hdr_formats.length === 0
    ? t("copies.value.sdr")
    : copy.media.hdr_formats.join(" · ");
}

function audioText(copy: CopyRow): string {
  if (copy.media === null) return t("copies.value.unread");
  return copy.media.audio === "" ? t("copies.value.none") : copy.media.audio;
}

function bitrateText(copy: CopyRow): string {
  if (copy.media === null) return t("copies.value.unread");
  return copy.media.video_bitrate_kbps === 0
    ? t("copies.value.unknown")
    : t("copies.value.bitrate", {
        value: (copy.media.video_bitrate_kbps / 1000).toFixed(1),
      });
}

/** The length of the video; "Unknown" when the file does not say. */
function lengthText(copy: CopyRow, runtime: CardFormat["runtime"]): string {
  if (copy.media === null) return t("copies.value.unread");
  const minutes = copy.media.duration_minutes;
  return minutes === 0 ? t("copies.value.unknown") : runtime(minutes);
}

/** The heading of a card: the resolution and the first HDR format. */
export function headingOf(copy: CopyRow): string {
  const media = copy.media;
  if (media === null) return t("copies.value.unread");
  const hdr = media.hdr_formats[0];
  const resolution = media.resolution === "" ? t("copies.value.unknown") : media.resolution;
  return hdr === undefined ? resolution : `${resolution} · ${hdr}`;
}

/** Whether a path is on a network share (a UNC path). */
export function isNetworkPath(path: string): boolean {
  return path.startsWith("\\\\") || path.startsWith("//");
}

export interface CardFormat {
  size: (bytes: number) => string;
  /** "1h 52m" from whole minutes. */
  runtime: (minutes: number) => string;
  /** Shortens a long path to fit a cell. */
  folder: (path: string) => string;
}

/** The cards of the page with every value, its verdict and the notes. */
export function cardsOf(page: Comparison, format: CardFormat): Card[] {
  const copies = page.copies;
  const verdictOf = {
    resolution: verdicts(copies.map(resolutionRank)),
    hdr: verdicts(copies.map(hdrRank)),
    audio: verdicts(copies.map(audioRank)),
    source: verdicts(copies.map(sourceRank)),
    bitrate: verdicts(copies.map(bitrateRank)),
  };
  const lengths = copies.map((copy) => copy.media?.duration_minutes ?? null);
  const lengthVerdict: Verdict = lengths.every((value) => value !== null && value === lengths[0])
    ? "same"
    : "plain";
  const lengthsDiffer = (lengthGap(page) ?? 0) > DIFFERENT_LENGTH_MINUTES;
  const largest = Math.max(...copies.map((copy) => copy.size_bytes ?? 0));

  return copies.map((copy, index) => {
    const verdict = (row: keyof typeof verdictOf): Verdict => verdictOf[row][index] ?? "plain";
    const size = copy.size_bytes ?? 0;
    const smaller = largest - size;
    const cells: Cell[] = [
      { row: "resolution", text: resolutionText(copy), verdict: verdict("resolution") },
      { row: "hdr", text: hdrText(copy), verdict: verdict("hdr") },
      { row: "audio", text: audioText(copy), verdict: verdict("audio") },
      {
        row: "source",
        text: copy.source === "" ? t("copies.value.unknown") : copy.source,
        verdict: verdict("source"),
      },
      { row: "bitrate", text: bitrateText(copy), verdict: verdict("bitrate") },
      {
        row: "length",
        text: lengthText(copy, format.runtime),
        verdict: lengthVerdict,
        ...(lengthsDiffer ? { warn: true } : {}),
      },
      {
        row: "size",
        text: format.size(size),
        verdict: "plain",
        ...(smaller > 0 ? { note: t("copies.value.smaller", { size: format.size(smaller) }) } : {}),
      },
      {
        row: "folder",
        text: format.folder(copy.folder),
        verdict: "plain",
        full: copy.folder,
        mono: true,
      },
    ];
    return {
      copy,
      heading: headingOf(copy),
      drive: copy.source_label,
      network: isNetworkPath(copy.folder),
      cells,
    };
  });
}

/** Lengths further apart than this many minutes may be different films. */
export const DIFFERENT_LENGTH_MINUTES = 5;

/** How many minutes the shortest and the longest known length are apart; `null` with fewer than two. */
export function lengthGap(page: Comparison): number | null {
  const known = page.copies
    .map((copy) => copy.media?.duration_minutes ?? 0)
    .filter((minutes) => minutes > 0);
  return known.length < 2 ? null : Math.max(...known) - Math.min(...known);
}

/** The minutes the lengths differ by when that is enough to be different films, else `null`. */
export function lengthWarning(page: Comparison): number | null {
  const gap = lengthGap(page);
  return gap !== null && gap > DIFFERENT_LENGTH_MINUTES ? gap : null;
}

/** The size of every copy together. */
export function totalBytes(page: Comparison): number {
  return page.copies.reduce((sum, copy) => sum + (copy.size_bytes ?? 0), 0);
}

/** What keeping the biggest copy frees: everything but it. */
export function freedBytes(page: Comparison): number {
  return totalBytes(page) - Math.max(...page.copies.map((copy) => copy.size_bytes ?? 0));
}
