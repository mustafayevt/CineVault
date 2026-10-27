import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";

/** One line of a file's stream table. */
export interface StreamRow {
  /** `Video`, `Audio 2`, `Subtitle 1`. */
  label: string;
  codec: string;
  /** Resolution, frame rate and bitrate for video; layout and bitrate for audio. */
  details: string;
  /** The language's name, or "" when unknown. */
  language: string;
  /** Words, never colour alone: `default`, `forced`, `SDH`, `Dolby Vision`. */
  flags: string[];
}

/** Subtitle formats that are pictures; every other one is text. */
const IMAGE_SUBTITLES = ["pgs", "vobsub", "dvd", "dvb", "hdmv", "xsub"];

export function isImageSubtitle(codec: string): boolean {
  const lower = codec.toLowerCase();
  return IMAGE_SUBTITLES.some((name) => lower.includes(name));
}

/** `46 Mb/s`, `8.4 Mb/s`, `640 kb/s`. */
export function bitrateLabel(kbps: number, format: Formatters): string {
  if (kbps >= 10_000)
    return t("detail.unit.mbps", { value: format.number(Math.round(kbps / 1000)) });
  if (kbps >= 1000) {
    return t("detail.unit.mbps", { value: format.number(Math.round(kbps / 100) / 10) });
  }
  return t("detail.unit.kbps", { value: format.number(kbps) });
}
