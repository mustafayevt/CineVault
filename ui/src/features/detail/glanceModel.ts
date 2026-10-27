import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import type { MovieDetail } from "./detailBackend";
import { languageName } from "./detailFormat";
import { heroBadges } from "./heroAudio";
import type { HeroBadge } from "./heroAudio";

/** The summed size of a movie's video files, or `null` when no size is known. */
function totalSize(detail: Pick<MovieDetail, "videos">): number | null {
  const sizes = detail.videos.flatMap((video) =>
    video.size_bytes === null ? [] : [video.size_bytes],
  );
  return sizes.length === 0 ? null : sizes.reduce((sum, size) => sum + size, 0);
}

/** What the Technical card reads from a title (a movie, or an episode's file facts). */
export type TechDetail = Pick<
  MovieDetail,
  | "tech"
  | "hero_audio"
  | "videos"
  | "subtitles"
  | "original_language"
  | "folder"
  | "data_source"
  | "source_label"
>;

/** The facets drawn as chips: the resolution and the HDR formats, the first thing to know. */
const PRIMARY_FACETS: ReadonlySet<string> = new Set(["video_format", "hdr_format"]);

/** The Technical card, in the words and numbers it shows. */
export interface TechSheet {
  /** The video details were never read: the Video row says so, once. */
  unread: boolean;
  /** Why they are missing (`read_problem`), for [`unreadText`]; "" while simply pending. */
  problem: string;
  video: {
    /** `3840×1608`, or "" while unknown. */
    dimensions: string;
    /** Resolution and HDR formats. */
    primary: HeroBadge[];
    /** Codec, bitrate, aspect ratio, source. */
    secondary: HeroBadge[];
  };
  audio: {
    /** The track the title leads with, with its language. */
    main: HeroBadge | null;
    /** How many tracks there are besides it. */
    more: number;
  };
  subtitles: {
    languages: string[];
    count: number;
  };
  file: {
    size: number | null;
    container: string;
    files: number;
  };
}

/** Unique values in their first-seen order. */
function unique(values: readonly string[]): string[] {
  return [...new Set(values)];
}

/** The spec sheet of a title: video, audio, subtitles and file, ready for the Technical card. */
export function techSheet(detail: TechDetail, format: Pick<Formatters, "locale">): TechSheet {
  const badges = heroBadges(detail, format.locale);
  const streams = detail.videos.flatMap((file) => file.streams);
  const video = streams.find((stream) => stream.kind === "video");
  const audioCount = streams.filter((stream) => stream.kind === "audio").length;
  const items = badges.items;
  const main = items.find((item) => item.badge.facet === "audio_codec") ?? null;
  const languages = unique(
    detail.subtitles
      .map((subtitle) => languageName(subtitle.language, format.locale))
      .map((name) => (name === "" ? t("detail.spec.unknownLanguage") : name)),
  );
  return {
    unread: badges.unread,
    problem: badges.problem,
    video: {
      dimensions:
        video !== undefined && video.width > 0 && video.height > 0
          ? `${video.width}×${video.height}`
          : "",
      primary: items.filter((item) => PRIMARY_FACETS.has(item.badge.facet)),
      secondary: items.filter(
        (item) => !PRIMARY_FACETS.has(item.badge.facet) && item.badge.facet !== "audio_codec",
      ),
    },
    audio: {
      main,
      more: Math.max(0, audioCount - (main === null ? 0 : 1)),
    },
    subtitles: { languages, count: detail.subtitles.length },
    file: {
      size: totalSize(detail),
      container: detail.videos[0]?.container ?? "",
      files: detail.videos.length,
    },
  };
}
