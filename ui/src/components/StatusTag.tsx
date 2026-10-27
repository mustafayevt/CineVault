import { Check, CircleDot, LoaderCircle, Minus, TriangleAlert, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ReactNode } from "react";

import { t } from "../i18n/catalog";
import type { MessageKey } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import type { Status } from "./status";

const glyphs: Record<Status, LucideIcon> = {
  done: Check,
  waiting: CircleDot,
  missing: X,
  "needs-key": TriangleAlert,
  "in-progress": LoaderCircle,
  off: Minus,
};

const words: Record<Status, MessageKey> = {
  done: "status.done",
  waiting: "status.waiting",
  missing: "status.missing",
  "needs-key": "status.needsKey",
  "in-progress": "status.inProgress",
  off: "status.off",
};

const tones: Record<Status, string> = {
  done: "bg-surface-active text-ok",
  waiting: "bg-gold-tint text-gold",
  missing: "bg-surface-active text-danger",
  "needs-key": "bg-warn-tint text-warn",
  "in-progress": "bg-surface-active text-text-2",
  // Off by choice has no ground at all, so it never reads as a problem.
  off: "text-text-muted",
};

interface StatusTagProps {
  status: Status;
  /** A more specific word than the default, for example "Trailer" or "Poster". */
  children?: ReactNode;
  className?: string;
  /**
   * `soft` shows a gap that is expected, such as a collection before its details are filled: the
   * same glyph and word on the amber ground instead of the red one.
   */
  tone?: "soft";
}

/**
 * A status as a glyph plus a word. Done is a check on green, waiting a dot on the accent, missing a
 * cross on red, needs-key a triangle on amber, in-progress a ring on blue and off a muted dash.
 * "Off" and "missing" look different on purpose: off is a choice, missing is a gap.
 */
export function StatusTag({ status, children, className, tone }: StatusTagProps) {
  return (
    <span
      data-status={status}
      className={cx(
        "type-small inline-flex h-6 items-center gap-1 rounded-(--r-4) px-2 whitespace-nowrap",
        tone === "soft" && status === "missing" ? "bg-warn-tint text-warn" : tones[status],
        className,
      )}
    >
      <Icon
        icon={glyphs[status]}
        size={14}
        className={status === "in-progress" ? "animate-spin" : undefined}
      />
      <span>{children ?? t(words[status])}</span>
    </span>
  );
}
