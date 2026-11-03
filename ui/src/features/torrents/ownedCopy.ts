import type {
  TorrentEpisodeRefDto,
  TorrentEpisodeSpanDto,
  TorrentLibraryCopyDto,
} from "../../bindings";
import { t } from "../../i18n/catalog";
import type { TitleDetails, TorrentRelease } from "./torrentBackend";

/** A movie's copy in the library, as the title details send it. */
export type LibraryCopy = TorrentLibraryCopyDto;

/** How a release compares with the copy: the backend's verdict (`cv_torrent::rank::upgrade`). */
export type Verdict = "better" | "same" | "worse";

/** The verdict a release carries, or null when it was not compared. */
export function verdictOf(release: Pick<TorrentRelease, "upgrade">): Verdict | null {
  const verdict = release.upgrade;
  return verdict === "better" || verdict === "same" || verdict === "worse" ? verdict : null;
}

/** "Dolby Vision" for the short HDR word the backend sends. */
export function hdrWord(hdr: string | null): string {
  return hdr === "DV" ? "Dolby Vision" : (hdr ?? "");
}

/** "2160p Dolby Vision", "1080p SDR"; the source too when that is all that tells them apart. */
function qualityOf(
  side: { resolution: string; hdr: string | null; source: string | null },
  withSource: boolean,
): string {
  return [
    side.resolution === "unknown" ? "" : side.resolution,
    side.hdr === null ? t("torrentsPage.cmp.sdr") : hdrWord(side.hdr),
    withSource ? (side.source ?? "") : "",
  ]
    .filter(Boolean)
    .join(" ");
}

/** "Better: 2160p Dolby Vision vs your 1080p SDR", the tooltip of a release's tag. */
export function reasonOf(verdict: Verdict, copy: LibraryCopy, release: TorrentRelease): string {
  const samePicture = copy.resolution === release.resolution && copy.hdr === release.hdr;
  return t("torrentsPage.cmp.why", {
    verdict: t(`torrentsPage.cmp.${verdict}`),
    theirs: qualityOf(release, samePicture),
    mine: qualityOf(copy, samePicture),
  });
}

/** How good an audio format is, from its name; 0 when unknown. */
export function audioRank(audio: string | null): number {
  const text = (audio ?? "").toLowerCase();
  if (text === "") return 0;
  if (text.includes("atmos")) return 7;
  if (text.includes("truehd")) return 6;
  if (/dts[-\s]?(hd|x)/.test(text)) return 5;
  if (text.includes("dts")) return 4;
  if (/dd\+|ddp|e-?ac-?3|eac3/.test(text)) return 3;
  if (/ac-?3|\bdd\b/.test(text)) return 2;
  if (/aac|mp3|opus|flac|pcm/.test(text)) return 1;
  return 0;
}

const EDITIONS: readonly [RegExp, string][] = [
  [/director'?s[ .]?cut/i, "Director's Cut"],
  [/\bextended\b/i, "Extended"],
  [/\btheatrical\b/i, "Theatrical"],
  [/\bunrated\b/i, "Unrated"],
  [/\buncut\b/i, "Uncut"],
  [/\bimax\b/i, "IMAX"],
  [/\bremastered\b/i, "Remastered"],
  [/\bcriterion\b/i, "Criterion"],
  [/final[ .]cut/i, "Final Cut"],
];

/** The edition a release name gives, or null. */
export function editionOf(name: string): string | null {
  const words = name.replace(/[._]/g, " ");
  return EDITIONS.find(([pattern]) => pattern.test(words))?.[1] ?? null;
}

/** Codec names as one vocabulary: x265, H.265 and HEVC are one codec. */
function codecKey(codec: string | null): string {
  const text = (codec ?? "").toLowerCase().replace(/[.\s-]/g, "");
  if (text === "") return "";
  if (/hevc|h265|x265/.test(text)) return "hevc";
  if (/avc|h264|x264/.test(text)) return "avc";
  return text;
}

export type CompareRowKey =
  "resolution" | "hdr" | "codec" | "audio" | "source" | "edition" | "size";

export interface CompareRow {
  key: CompareRowKey;
  mine: string;
  theirs: string;
  /** Both sides are known and they are not the same. */
  differs: boolean;
}

/** "2.3", "0.4", "12": a size ratio with one decimal under ten. */
function ratioText(ratio: number): string {
  return ratio >= 10 ? String(Math.round(ratio)) : String(Math.round(ratio * 10) / 10);
}

/**
 * The rows of "Compare with yours": the copy and the release side by side, resolution, HDR,
 * codec, audio, source, edition and size, with the rows that differ marked. A side the name or
 * the media info does not give is a dash and never counts as a difference.
 */
export function compareRows(
  copy: LibraryCopy,
  release: TorrentRelease,
  size: (bytes: number) => string,
): CompareRow[] {
  const dash = t("torrentsPage.cmp.unknown");
  const shown = (value: string | null | undefined) =>
    value === null || value === undefined || value === "" || value === "unknown" ? dash : value;
  const row = (
    key: CompareRowKey,
    mine: string | null,
    theirs: string | null,
    same: (a: string, b: string) => boolean = (a, b) => a.toLowerCase() === b.toLowerCase(),
  ): CompareRow => {
    const left = shown(mine);
    const right = shown(theirs);
    return {
      key,
      mine: left,
      theirs: right,
      differs: left !== dash && right !== dash && !same(left, right),
    };
  };
  const sdr = t("torrentsPage.cmp.sdr");
  const theirSize = release.size;
  return [
    row("resolution", copy.resolution, release.resolution),
    {
      key: "hdr",
      mine: copy.hdr === null ? sdr : hdrWord(copy.hdr),
      theirs: release.hdr === null ? sdr : hdrWord(release.hdr),
      differs: (copy.hdr ?? "") !== (release.hdr ?? ""),
    },
    row("codec", copy.codec, release.codec, (a, b) => codecKey(a) === codecKey(b)),
    row("audio", copy.audio, release.audio, (a, b) => audioRank(a) === audioRank(b)),
    row("source", copy.source, release.source),
    {
      key: "edition",
      mine: copy.edition ?? dash,
      theirs: editionOf(release.name) ?? dash,
      differs: (copy.edition ?? "").toLowerCase() !== (editionOf(release.name) ?? "").toLowerCase(),
    },
    {
      key: "size",
      mine: (copy.size ?? 0) > 0 ? size(copy.size ?? 0) : dash,
      theirs: theirSize === null ? dash : size(theirSize),
      differs:
        theirSize !== null &&
        (copy.size ?? 0) > 0 &&
        Math.abs(theirSize / (copy.size ?? 0) - 1) >= 0.1,
    },
  ];
}

/**
 * The plain verdict under the rows: "Better picture, same audio, 2.3× the size". The picture
 * word is the backend's verdict, so it never disagrees with the tag; the audio part is left out
 * when either side's audio is unknown, the size part when either size is.
 */
export function verdictSentence(
  verdict: Verdict,
  copy: LibraryCopy,
  release: TorrentRelease,
): string {
  const parts = [t(`torrentsPage.cmp.picture.${verdict}`)];
  const mine = audioRank(copy.audio);
  const theirs = audioRank(release.audio);
  if (mine > 0 && theirs > 0) {
    const audio = theirs > mine ? "better" : theirs < mine ? "worse" : "same";
    parts.push(t(`torrentsPage.cmp.audio.${audio}`));
  }
  if (release.size !== null && (copy.size ?? 0) > 0) {
    parts.push(t("torrentsPage.cmp.size", { ratio: ratioText(release.size / (copy.size ?? 0)) }));
  }
  return parts.join(", ");
}

/** The facts of a copy on one line: "2160p · Dolby Vision · HEVC · TrueHD Atmos 7.1 · Remux". */
export function copyFacts(copy: LibraryCopy): string[] {
  return [
    copy.resolution === "unknown" ? "" : copy.resolution,
    hdrWord(copy.hdr),
    copy.codec ?? "",
    copy.audio ?? "",
    copy.source ?? "",
    copy.edition ?? "",
  ].filter(Boolean);
}

/** Whether the library holds anything of the title to show the card for. */
export function holdsTitle(details: TitleDetails): boolean {
  if (details.inLibrary === null) return false;
  return details.kind === "show"
    ? details.heldEpisodes.length > 0
    : details.libraryCopies.length > 0;
}

// ---------------------------------------------------------------- shows

/** What a show release is against the episodes the library holds. */
export type ShowMark = "gap" | "held" | "better";

const key = (season: number, episode: number) => `${season}:${episode}`;

/** The episodes the library holds and misses of a show, for marking its releases. */
export interface ShowHoldings {
  held: ReadonlySet<string>;
  missing: ReadonlySet<string>;
  /** Every known episode (held or missing) by season. */
  bySeason: ReadonlyMap<number, readonly number[]>;
}

export function showHoldings(
  held: readonly TorrentEpisodeRefDto[],
  missing: readonly TorrentEpisodeRefDto[],
): ShowHoldings {
  const bySeason = new Map<number, number[]>();
  for (const episode of [...held, ...missing]) {
    const list = bySeason.get(episode.season) ?? [];
    if (!list.includes(episode.episode)) list.push(episode.episode);
    bySeason.set(episode.season, list);
  }
  return {
    held: new Set(held.map((e) => key(e.season, e.episode))),
    missing: new Set(missing.map((e) => key(e.season, e.episode))),
    bySeason,
  };
}

/** The known episodes a release covers, as keys. */
function covered(span: TorrentEpisodeSpanDto, holdings: ShowHoldings): string[] {
  const seasons = [...holdings.bySeason.keys()].filter(
    (season) =>
      (span.firstSeason === null || season >= span.firstSeason) &&
      (span.lastSeason === null || season <= span.lastSeason),
  );
  if (span.firstSeason !== null && span.firstEpisode !== null) {
    const last = span.lastEpisode ?? span.firstEpisode;
    const out: string[] = [];
    for (let episode = span.firstEpisode; episode <= last; episode += 1) {
      out.push(key(span.firstSeason, episode));
    }
    return out;
  }
  return seasons.flatMap((season) =>
    (holdings.bySeason.get(season) ?? []).map((episode) => key(season, episode)),
  );
}

/**
 * "Fills a gap" when a release covers an episode the library is missing, "In your library" when
 * every known episode it covers is held, and "Better" when they are held and the backend found
 * it better than the episodes' quality. Null when it covers no episode the library knows of.
 */
export function showMarkOf(release: TorrentRelease, holdings: ShowHoldings): ShowMark | null {
  if (release.episodes === null) return null;
  const known = covered(release.episodes, holdings).filter(
    (episode) => holdings.held.has(episode) || holdings.missing.has(episode),
  );
  if (known.length === 0) return null;
  if (known.some((episode) => holdings.missing.has(episode))) return "gap";
  return verdictOf(release) === "better" ? "better" : "held";
}

/** "S01–S03", "S01, S03": the seasons in which the library holds an episode. */
export function seasonsText(held: readonly TorrentEpisodeRefDto[]): string {
  const seasons = [...new Set(held.map((e) => e.season))].sort((a, b) => a - b);
  const two = (n: number) => String(n).padStart(2, "0");
  const runs: string[] = [];
  let start = 0;
  for (let index = 1; index <= seasons.length; index += 1) {
    if (index === seasons.length || seasons[index] !== (seasons[index - 1] ?? 0) + 1) {
      const first = seasons[start] ?? 0;
      const last = seasons[index - 1] ?? 0;
      runs.push(first === last ? `S${two(first)}` : `S${two(first)}–S${two(last)}`);
      start = index;
    }
  }
  return runs.join(", ");
}

/** "S04E05–E08, S05E01": the missing episodes as runs, at most `limit` of them. */
export function missingText(missing: readonly TorrentEpisodeRefDto[], limit = 3): string {
  const sorted = [...missing].sort((a, b) => a.season - b.season || a.episode - b.episode);
  const two = (n: number) => String(n).padStart(2, "0");
  const runs: string[] = [];
  let start = 0;
  for (let index = 1; index <= sorted.length; index += 1) {
    const previous = sorted[index - 1];
    const current = sorted[index];
    if (
      current === undefined ||
      previous === undefined ||
      current.season !== previous.season ||
      current.episode !== previous.episode + 1
    ) {
      const first = sorted[start];
      if (first !== undefined && previous !== undefined) {
        const head = `S${two(first.season)}E${two(first.episode)}`;
        runs.push(first.episode === previous.episode ? head : `${head}–E${two(previous.episode)}`);
      }
      start = index;
    }
  }
  if (runs.length <= limit) return runs.join(", ");
  return t("torrentsPage.have.more", {
    list: runs.slice(0, limit).join(", "),
    count: runs.length - limit,
  });
}
