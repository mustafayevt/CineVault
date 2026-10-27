/**
 * The Audit lens: per-title completeness and a technical summary. The pips are the four things a
 * title is expected to have next to its video. Missing ones are never marked by colour alone: each
 * pip has an icon (check or ×) and its name, and the card's accessible name lists what is missing.
 */
export type PipId = "nfo" | "poster" | "fanart" | "logo";

/** What the Audit lens shows for one title. */
export interface AuditSummary {
  present: Readonly<Record<PipId, boolean>>;
  /** Resolution, HDR and audio on one mono line, or "" when none of them is known. */
  tech: string;
  /** The title has a video file (its media info may still be pending). */
  hasVideo: boolean;
}

/** The speaker layout of a channel count: 8 channels are 7.1, 6 are 5.1, 2 are 2.0. */
export function channelLayout(channels: number): string {
  if (channels <= 0) return "";
  if (channels === 1) return "1.0";
  if (channels === 2) return "2.0";
  return `${channels - 1}.1`;
}
