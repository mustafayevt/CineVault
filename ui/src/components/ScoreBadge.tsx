import { cx } from "../lib/cx";
import { tagBase } from "./tagStyles";

/** How sure a match score is: `ok` (confident), `warn` (uncertain), `bad` (unlikely). */
export type ScoreTone = "ok" | "warn" | "bad";

const TONES: Record<ScoreTone, string> = {
  ok: "bg-[color-mix(in_srgb,var(--ok)_14%,transparent)] text-ok",
  warn: "bg-warn-tint text-warn",
  bad: "bg-[color-mix(in_srgb,var(--danger)_14%,transparent)] text-danger",
};

interface ScoreBadgeProps {
  /** The rating as shown: 8.1, "81%", "A-". */
  value: number | string;
  /** Names the source for assistive technology: "IMDb". The badge then reads "IMDb 8.1". */
  source?: string;
  /** Colours the badge by how sure a match score is; left out, it is neutral (a rating). */
  tone?: ScoreTone;
  className?: string;
}

/** A rating in the tag box, 11/700 with tabular figures (700 is for numbers). */
export function ScoreBadge({ value, source, tone, className }: ScoreBadgeProps) {
  return (
    <span
      aria-label={source === undefined ? undefined : `${source} ${value}`}
      data-tone={tone}
      className={cx(
        tagBase,
        "font-(weight:--fw-700)",
        tone === undefined ? "bg-surface-active text-text-2" : TONES[tone],
        className,
      )}
    >
      {value}
    </span>
  );
}
