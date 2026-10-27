import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import { channelLayout } from "../library/audit";
import type { DetailStream, MovieDetail } from "./detailBackend";
import { languageName } from "./detailFormat";
import { bitrateLabel, isImageSubtitle } from "./streams";

/** One flag of a stream, in words (never colour alone). */
export interface FlagData {
  id: string;
  label: string;
  /** How it is drawn: the original track and commentary stand out from the rest. */
  tone: "accent" | "info" | "outline" | "neutral";
}

const FLAGS: Readonly<Record<string, Pick<FlagData, "tone"> & { label: Parameters<typeof t>[0] }>> =
  {
    default: { label: "media.flag.default", tone: "neutral" },
    forced: { label: "media.flag.forced", tone: "info" },
    hearing_impaired: { label: "media.flag.hearingImpaired", tone: "info" },
    original: { label: "media.flag.original", tone: "accent" },
    comment: { label: "media.flag.comment", tone: "outline" },
    dub: { label: "media.flag.dub", tone: "outline" },
    visual_impaired: { label: "media.flag.visualImpaired", tone: "outline" },
    lyrics: { label: "media.flag.lyrics", tone: "outline" },
    karaoke: { label: "media.flag.karaoke", tone: "outline" },
  };

/** The order flags are listed in, so the same flag is always in the same place. */
const FLAG_ORDER = [
  "default",
  "original",
  "forced",
  "hearing_impaired",
  "comment",
  "dub",
  "visual_impaired",
  "lyrics",
  "karaoke",
];

/** The flags of a stream as words: Default, Original, Forced, SDH, Commentary, ... */
function flagsOf(flags: readonly string[]): FlagData[] {
  const known = FLAG_ORDER.filter((id) => flags.includes(id));
  const other = flags.filter((id) => !Object.hasOwn(FLAGS, id));
  return [
    ...known.map((id): FlagData => {
      const spec = FLAGS[id];
      return { id, label: spec === undefined ? id : t(spec.label), tone: spec?.tone ?? "outline" };
    }),
    ...other.map((id): FlagData => ({ id, label: id.replaceAll("_", " "), tone: "outline" })),
  ];
}

/** The columns every row shares: where the stream is, and the file it belongs to. */
interface RowBase {
  key: string;
  /** The name of the video file the stream is in (shown when the title has several). */
  file: string;
}

export interface VideoRow extends RowBase {
  format: string;
  /** `3840×1608`, or "" while the size is unknown. */
  resolution: string;
  hdr: string[];
  flags: FlagData[];
  /** Technical level. */
  bitDepth: string;
  frameRate: string;
  bitrate: string;
}

export interface AudioRow extends RowBase {
  number: number;
  /** The language's name, or "" when unknown. */
  language: string;
  format: string;
  /** `7.1`. */
  channels: string;
  name: string;
  flags: FlagData[];
  /** The track the audio badge describes. */
  hero: boolean;
  /** Technical level. */
  layout: string;
  bitrate: string;
  /** `48 kHz`, or "" while unknown. */
  sampleRate: string;
  bitDepth: string;
}

export interface SubtitleRowData extends RowBase {
  source: "embedded" | "file";
  /** The position among the embedded streams, from 1; 0 for a file. */
  number: number;
  language: string;
  flags: FlagData[];
  /** `SubRip`, `PGS`, `Text`, ... */
  format: string;
  /** Picture subtitles (PGS, VobSub) cannot be edited as text. */
  image: boolean;
  /** The name of an external file; "" for an embedded stream. */
  fileName: string;
  name: string;
  /** The language code as the backend sends it, "" when the stream does not say. */
  languageCode: string;
}

/** The streams of a movie by type, ready for the tables of the Media tab. */
export interface MediaRows {
  video: VideoRow[];
  audio: AudioRow[];
  subtitles: SubtitleRowData[];
}

function streamsOf(detail: Pick<MovieDetail, "videos">, kind: string) {
  return detail.videos.flatMap((video) =>
    video.streams.filter((stream) => stream.kind === kind).map((stream) => ({ video, stream })),
  );
}

function frameRate(stream: DetailStream, format: Formatters): string {
  return stream.frame_rate !== null && stream.frame_rate > 0
    ? t("detail.unit.fps", { value: format.number(Math.round(stream.frame_rate * 1000) / 1000) })
    : "";
}

function sampleRate(stream: DetailStream, format: Formatters): string {
  return stream.sample_rate_hz > 0
    ? t("detail.unit.kHz", { value: format.number(stream.sample_rate_hz / 1000) })
    : "";
}

function bitDepth(stream: DetailStream): string {
  return stream.bit_depth > 0 ? t("detail.unit.bit", { value: stream.bit_depth }) : "";
}

/**
 * Every stream of the movie under its type. The subtitle list is the merged one: embedded streams
 * and the files next to the video together, each tagged Embedded or File with the language the
 * backend read from the stream or the file name. The hero audio track is marked on the first file
 * that has audio, the one the badge is read from.
 */
export function mediaRows(
  detail: Pick<MovieDetail, "videos" | "hero_audio" | "subtitles">,
  format: Formatters,
): MediaRows {
  const video = streamsOf(detail, "video").map(({ video: file, stream }, index): VideoRow => {
    return {
      key: `${file.name}:video:${index}`,
      file: file.name,
      format: stream.codec,
      resolution: stream.width > 0 && stream.height > 0 ? `${stream.width}×${stream.height}` : "",
      hdr: stream.hdr,
      flags: flagsOf(stream.flags),
      bitDepth: bitDepth(stream),
      frameRate: frameRate(stream, format),
      bitrate: stream.bitrate_kbps > 0 ? bitrateLabel(stream.bitrate_kbps, format) : "",
    };
  });

  const heroFile = detail.videos.find((file) => file.streams.some((s) => s.kind === "audio"));
  const hero = detail.hero_audio;
  const audio: AudioRow[] = [];
  for (const file of detail.videos) {
    let number = 0;
    for (const stream of file.streams) {
      if (stream.kind !== "audio") continue;
      number += 1;
      audio.push({
        key: `${file.name}:audio:${number}`,
        file: file.name,
        number,
        language: languageName(stream.language, format.locale),
        format: stream.codec,
        channels: channelLayout(stream.channels),
        name: stream.title,
        flags: flagsOf(stream.flags),
        hero: hero !== null && file === heroFile && number - 1 === hero.index,
        layout: stream.channels > 0 ? t("detail.unit.channels", { count: stream.channels }) : "",
        bitrate: stream.bitrate_kbps > 0 ? bitrateLabel(stream.bitrate_kbps, format) : "",
        sampleRate: sampleRate(stream, format),
        bitDepth: bitDepth(stream),
      });
    }
  }

  // The backend lists the embedded streams first, then the files, in `detail.subtitles`.
  const firstFile = detail.videos[0]?.name ?? "";
  let embedded = 0;
  const subtitles = detail.subtitles.map((subtitle, index): SubtitleRowData => {
    const isFile = subtitle.file_name !== null;
    if (!isFile) embedded += 1;
    return {
      key: `${subtitle.file_name ?? "embedded"}:${index}`,
      file: firstFile,
      source: isFile ? "file" : "embedded",
      number: isFile ? 0 : embedded,
      language: languageName(subtitle.language, format.locale),
      flags: flagsOf(subtitle.flags),
      format: subtitle.codec,
      image: isImageSubtitle(subtitle.codec),
      fileName: subtitle.file_name ?? "",
      name: subtitle.title,
      languageCode: subtitle.language,
    };
  });
  return { video, audio, subtitles };
}
