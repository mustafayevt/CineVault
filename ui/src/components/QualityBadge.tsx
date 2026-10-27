import { cx } from "../lib/cx";
import { tagBase } from "./tagStyles";

/** The resolution tiers the library shows. */
export type QualityTier = "2160p" | "1080p" | "720p" | "SD";

interface QualityBadgeProps {
  quality: QualityTier;
  /** The HDR word after the resolution: `true` for "HDR", or the format ("Dolby Vision", "HDR10+"). */
  hdr?: boolean | string;
  className?: string;
}

/**
 * The one resolution badge, the same in the grid, the table, the detail page and Torrents: the
 * neutral tag style with the resolution and, when it has one, the HDR word. No tier gets a colour.
 */
export function QualityBadge({ quality, hdr, className }: QualityBadgeProps) {
  const hdrWord = hdr === true ? "HDR" : typeof hdr === "string" && hdr !== "" ? hdr : null;
  return (
    <span
      data-quality={quality}
      className={cx(tagBase, "bg-surface-active text-text-2", className)}
    >
      {quality}
      {hdrWord !== null && <span className="text-text">{hdrWord}</span>}
    </span>
  );
}
