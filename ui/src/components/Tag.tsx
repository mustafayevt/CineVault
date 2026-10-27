import type { ReactNode } from "react";

import { cx } from "../lib/cx";
import { tagBase } from "./tagStyles";

/** `neutral`, `ok` and `warn` are the three looks. */
export type TagVariant = "neutral" | "ok" | "warn";

const variants: Record<TagVariant, string> = {
  neutral: "bg-surface-active text-text-2",
  ok: "bg-[color-mix(in_srgb,var(--ok)_14%,transparent)] text-ok",
  warn: "bg-warn-tint text-warn",
};

interface TagProps {
  variant?: TagVariant;
  className?: string;
  children: ReactNode;
}

/**
 * A small label (genres, keywords, states): 18px tall, 11/600, radius 4. Never an outlined pill.
 * Use `QualityBadge` for 2160p/1080p, `ScoreBadge` for a rating and `StageBadge` for a stage.
 */
export function Tag({ variant = "neutral", className, children }: TagProps) {
  return (
    <span data-variant={variant} className={cx(tagBase, variants[variant], className)}>
      {children}
    </span>
  );
}
