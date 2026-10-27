import { t } from "../../i18n/catalog";
import { languageName as nameOf } from "../edit/names";
import type { MovieDetail, TechBadge } from "./detailBackend";
import { languageName } from "./detailFormat";

/** What the audio badge and the technical badges read of a title. */
export interface HeroDetail {
  tech: readonly TechBadge[];
  hero_audio?: MovieDetail["hero_audio"];
  original_language?: string;
  videos?: readonly { analysed: boolean; read_problem: string }[];
}

/** The facets of the badges that describe the video file itself. */
const VIDEO_FACETS: ReadonlySet<string> = new Set([
  "video_format",
  "hdr_format",
  "video_codec",
  "video_bitrate",
  "aspect_ratio",
  "audio_codec",
]);

/** One badge of the hero: the text it shows, the badge it filters by and its tooltip. */
export interface HeroBadge {
  badge: TechBadge;
  text: string;
  /** Replaces the plain "Filter library by ..." tooltip (the audio badge names its track). */
  hint?: string;
}

export interface HeroBadges {
  /** The video details have not been read: the badges of the file are not known yet. */
  unread: boolean;
  /** Why, when a read failed or the video is a disc folder (`read_problem`); "" otherwise. */
  problem: string;
  items: HeroBadge[];
}

/** True when two language codes or names say the same language (`en` and `eng`). */
export function sameLanguage(a: string, b: string): boolean {
  if (a.trim() === "" || b.trim() === "") return false;
  return nameOf(a).toLowerCase() === nameOf(b).toLowerCase();
}

/**
 * Why the audio badge describes the track it does, in the order the backend chooses it: the track
 * flagged original, else the first one in the title's original language, else the default
 * track, else the first. Commentary tracks are never chosen while another track exists.
 */
export function heroAudioWhy(
  audio: NonNullable<HeroDetail["hero_audio"]>,
  originalLanguage: string,
  locale: string,
): string {
  const number = audio.index + 1;
  const language = languageName(audio.language, locale);
  let why: string;
  if (audio.flags.includes("original")) {
    why = t("detail.audio.why.original", {
      number,
      language: language === "" ? t("detail.audio.unknownLanguage") : language,
    });
  } else if (sameLanguage(audio.language, originalLanguage)) {
    why = t("detail.audio.why.language", { number, language });
  } else if (audio.flags.includes("default")) {
    why = t("detail.audio.why.default", { number });
  } else {
    why = t("detail.audio.why.first", { number });
  }
  return `${why} ${t("detail.audio.skips")}`;
}

/**
 * Why the video details are missing, in plain words: the reason of the first video that was not
 * read (`read_problem`), or "not read" while the read is only pending.
 */
export function unreadText(problem: string): string {
  switch (problem) {
    case "timeout":
      return t("detail.tech.timedOut");
    case "disc":
      return t("detail.tech.disc");
    case "open":
      return t("detail.tech.cannotOpen");
    case "unreadable":
      return t("detail.tech.unreadable");
    default:
      return t("detail.tech.notRead");
  }
}

/**
 * The technical badges of the hero. The audio badge describes the title's hero audio track and
 * names its language ("TrueHD Atmos 7.1 · English"), and its tooltip says which track that is
 * and why. A title whose video details were never read has no file badges: the hero says so.
 */
export function heroBadges(
  detail: Pick<HeroDetail, "tech" | "hero_audio" | "original_language" | "videos">,
  locale: string,
): HeroBadges {
  const videos = detail.videos;
  const unread = videos !== undefined && videos.length > 0 && videos.every((v) => !v.analysed);
  const problem = unread ? (videos?.find((v) => v.read_problem !== "")?.read_problem ?? "") : "";
  const items: HeroBadge[] = [];
  for (const badge of detail.tech) {
    if (unread && VIDEO_FACETS.has(badge.facet)) continue;
    const audio = detail.hero_audio ?? null;
    if (badge.facet === "audio_codec" && audio !== null) {
      const language = languageName(audio.language, locale);
      items.push({
        badge,
        text: language === "" ? audio.label : `${audio.label} · ${language}`,
        hint: `${heroAudioWhy(audio, detail.original_language ?? "", locale)} ${t("detail.tech.filter", { label: badge.label })}`,
      });
    } else {
      items.push({ badge, text: badge.label });
    }
  }
  return { unread, problem, items };
}
