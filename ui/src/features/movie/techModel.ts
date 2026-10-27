import type { DetailStreamDto, DetailVideoDto } from "../../bindings";
import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import type { MovieDetail } from "../detail/detailBackend";
import { techSheet } from "../detail/glanceModel";
import type { TechSheet } from "../detail/glanceModel";
import { mediaRows } from "../detail/mediaModel";
import type { AudioRow, SubtitleRowData } from "../detail/mediaModel";

/** The Technical card in words: what each row shows. */
export interface TechSummary {
  sheet: TechSheet;
  /** `2160p · Dolby Vision · HDR10`. */
  videoHeadline: string;
  /** `HEVC Main 10 · 3840×1608 · 2.39:1 · 23.976 fps · 46 Mb/s`. */
  videoDetail: string;
  audio: AudioRow[];
  subtitles: SubtitleRowData[];
  /** The summed size of the video files, or `null` when none is known. */
  size: number | null;
  container: string;
  /** Files the title has: its videos and the files next to them. */
  fileCount: number;
  /** The name of the main file (the first video). */
  fileName: string;
}

/**
 * The HDR formats of a video stream as words: Dolby Vision names its profile when the file says
 * it (`Dolby Vision Profile 8.1 · HDR10`). Empty for SDR.
 */
export function hdrLabel(stream: Pick<DetailStreamDto, "hdr" | "hdr_details">): string {
  const profile = stream.hdr_details?.dolby_vision_profile ?? "";
  return stream.hdr
    .map((name) =>
      name === "Dolby Vision" && profile !== ""
        ? `${name} ${t("movie.tech.hdr.profile", { profile })}`
        : name,
    )
    .join(" · ");
}

/** One line of the HDR section of the expanded view. */
export interface HdrFact {
  label: string;
  value: string;
}

/**
 * What the stream says about its HDR beyond the format names: the Dolby Vision profile and base
 * layer, the mastering display (primaries and luminance range) and MaxCLL and MaxFALL. Only what
 * the file states is listed, so an SDR stream, or one read before these were kept, has none.
 */
export function hdrFacts(
  stream: Pick<DetailStreamDto, "hdr_details">,
  format: Formatters,
): HdrFact[] {
  const details = stream.hdr_details;
  if (details === null) return [];
  const nits = (value: number) =>
    t("movie.tech.hdr.nits", { value: format.number(Math.round(value * 10_000) / 10_000) });
  const facts: HdrFact[] = [];
  if (details.dolby_vision_profile !== "") {
    const base =
      details.dolby_vision_compatibility === ""
        ? ""
        : ` · ${t("movie.tech.hdr.baseLayer", { format: details.dolby_vision_compatibility })}`;
    facts.push({
      label: t("movie.tech.hdr.dolbyVision"),
      value: `${t("movie.tech.hdr.profile", { profile: details.dolby_vision_profile })}${base}`,
    });
  }
  const { mastering_min_nits: min, mastering_max_nits: max } = details;
  const luminance =
    min !== null && max !== null
      ? t("movie.tech.hdr.luminance", {
          min: format.number(Math.round(min * 10_000) / 10_000),
          max: format.number(Math.round(max * 10_000) / 10_000),
        })
      : "";
  const mastering = [details.mastering_primaries, luminance].filter((part) => part !== "");
  if (mastering.length > 0) {
    facts.push({ label: t("movie.tech.hdr.mastering"), value: mastering.join(" · ") });
  }
  if (details.max_cll_nits !== null) {
    facts.push({ label: t("movie.tech.hdr.maxCll"), value: nits(details.max_cll_nits) });
  }
  if (details.max_fall_nits !== null) {
    facts.push({ label: t("movie.tech.hdr.maxFall"), value: nits(details.max_fall_nits) });
  }
  return facts;
}

/** Sums the sizes of video files that have one. */
export function totalVideoSize(videos: readonly DetailVideoDto[]): number | null {
  const sizes = videos.flatMap((video) => (video.size_bytes === null ? [] : [video.size_bytes]));
  return sizes.length === 0 ? null : sizes.reduce((sum, size) => sum + size, 0);
}

/** The summary the Technical card shows for a movie. */
export function techSummary(detail: MovieDetail, format: Formatters): TechSummary {
  const sheet = techSheet(detail, format);
  const rows = mediaRows(detail, format);
  const videoRow = rows.video[0];
  const detailParts = [
    sheet.video.dimensions,
    videoRow?.frameRate ?? "",
    ...sheet.video.secondary.map((item) => item.text),
  ].filter((part) => part !== "");
  return {
    sheet,
    videoHeadline: sheet.video.primary.map((item) => item.text).join(" · "),
    videoDetail: detailParts.join(" · "),
    audio: rows.audio,
    subtitles: rows.subtitles,
    size: totalVideoSize(detail.videos),
    container: detail.videos[0]?.container ?? "",
    fileCount: detail.videos.length + detail.files.length,
    fileName: detail.videos[0]?.name ?? "",
  };
}

/** `46.9 Mb/s` from a size in bytes and a duration in seconds; empty when either is unknown. */
export function overallBitrate(bytes: number | null, seconds: number, format: Formatters): string {
  if (bytes === null || bytes <= 0 || seconds <= 0) return "";
  const mbps = (bytes * 8) / seconds / 1_000_000;
  return `${format.number(Math.round(mbps * 10) / 10)} Mb/s`;
}

/** `2h 46m 12s`; empty for an unknown duration. */
export function durationText(seconds: number): string {
  if (seconds <= 0) return "";
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  const rest = Math.round(seconds % 60);
  return [hours > 0 ? `${hours}h` : "", hours > 0 || minutes > 0 ? `${minutes}m` : "", `${rest}s`]
    .filter((part) => part !== "")
    .join(" ");
}
