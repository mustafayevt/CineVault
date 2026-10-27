import { X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { Children, useEffect, useRef, useState } from "react";
import type { CSSProperties, ReactNode } from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { Button } from "./Button";
import { IconButton } from "./IconButton";
import { Icon } from "./Icon";
import { Kbd } from "./Kbd";
import { Presence } from "./Presence";
import "./toast.css";

/** How long a toast stays visible when nothing hovers or focuses it. */
export const TOAST_DURATION_MS = 6000;

/** A toast that offers Undo stays about 8 seconds, long enough to read and reach for it. */
export const UNDO_TOAST_DURATION_MS = 8000;

/** The most toasts the stack shows at once. */
const TOAST_STACK_MAX = 3;

interface ToastProps {
  /** A past-tense statement with a count: "Marked 12 watched". */
  message: string;
  /** The glyph of the action the toast confirms (FolderPen for Rename, RefreshCw for Scrape). */
  glyph?: LucideIcon;
  /** Shows the Undo text button when set. */
  onUndo?: () => void;
  /** Shortcut hint next to Undo; an empty string hides it (the action has no binding). */
  undoShortcut?: string;
  /** How many identical toasts this one stands for; shown as "×3" from 2 up. */
  count?: number;
  /** `error` is for a failure that needs action: a danger dot, and `action` links to the detail. */
  tone?: "default" | "error";
  /** A text button after the message, for example "View in Activity". */
  action?: { label: string; onClick: () => void };
  /** Called by the dismiss button and when the timer runs out. */
  onDismiss?: () => void;
  /** Visible time in ms; `null` keeps the toast until dismissed. Paused while hovered or focused. */
  duration?: number | null;
  /** Whether the toast is on screen; turning it false plays the exit (the stack removes it after). */
  open?: boolean;
  className?: string;
}

/**
 * Confirms a completed action and offers Undo. A floating surface at radius 10 that slides in from
 * the bottom right (`.cv-slide-up`), with the action's glyph, the message, an optional "×N" for
 * merged toasts, Undo with its key hint, a dismiss button and a thin time bar that pauses while
 * the pointer or focus is on the toast. Toasts replace confirmation dialogs.
 */
export function Toast({
  message,
  glyph,
  onUndo,
  count = 1,
  tone = "default",
  action,
  undoShortcut = "Ctrl Z",
  onDismiss,
  duration = TOAST_DURATION_MS,
  open = true,
  className,
}: ToastProps) {
  const [paused, setPaused] = useState(false);
  // Time left on the clock: a pause keeps what was left, so the time bar and the timer agree.
  const remaining = useRef(duration ?? 0);

  useEffect(() => {
    remaining.current = duration ?? 0;
  }, [duration]);

  useEffect(() => {
    if (duration === null || paused || !onDismiss || !open) return;
    const started = Date.now();
    const timer = window.setTimeout(onDismiss, remaining.current);
    return () => {
      window.clearTimeout(timer);
      remaining.current = Math.max(0, remaining.current - (Date.now() - started));
    };
  }, [duration, paused, onDismiss, open]);

  return (
    <Presence present={open}>
      {(presence) => (
        <div
          {...presence}
          role="status"
          onMouseEnter={() => setPaused(true)}
          onMouseLeave={() => setPaused(false)}
          onFocus={() => setPaused(true)}
          onBlur={() => setPaused(false)}
          style={{ "--cv-slide-x": "16px", "--cv-slide-y": "8px" } as CSSProperties}
          className={cx(
            "cv-slide-up relative inline-flex items-center gap-2.5 overflow-hidden rounded-(--r-10)",
            "border border-solid border-line-edge bg-surface-floating py-1.5 pr-1.5 pl-3",
            "text-(length:--fs-13) text-text shadow-floating",
            className,
          )}
        >
          {tone === "error" ? (
            <i
              aria-hidden="true"
              className="size-1.5 shrink-0 rounded-(--r-pill) bg-danger"
              data-testid="toast-error-dot"
            />
          ) : glyph !== undefined ? (
            <Icon icon={glyph} size={16} className="shrink-0 text-text-2" />
          ) : null}
          <span className="font-(weight:--fw-500)">
            {message}
            {count > 1 && (
              <span className="ml-2 font-(weight:--fw-600) text-text-muted tabular-nums">
                {t("toast.repeat", { count })}
              </span>
            )}
          </span>
          {action && (
            <Button variant="ghost" size="sm" onClick={action.onClick}>
              {action.label}
            </Button>
          )}
          {onUndo && (
            <Button variant="ghost" size="sm" onClick={onUndo} className="gap-2 text-text">
              {t("common.undo")}
              {undoShortcut ? (
                <span aria-hidden="true">
                  <Kbd>{undoShortcut}</Kbd>
                </span>
              ) : null}
            </Button>
          )}
          <IconButton aria-label={t("common.dismiss")} icon={X} size="sm" onClick={onDismiss} />
          {duration !== null && onDismiss ? (
            <span
              aria-hidden="true"
              data-testid="toast-time"
              className="cv-toast-bar pointer-events-none absolute inset-x-0 bottom-0 h-[2px] bg-bar-neutral"
              style={{
                animationDuration: `${duration}ms`,
                animationPlayState: paused ? "paused" : "running",
              }}
            />
          ) : null}
        </div>
      )}
    </Presence>
  );
}

interface ToastStackProps {
  /** The toasts, oldest first; only the newest few show and the newest sits lowest. */
  children: ReactNode;
  /** Most toasts at once (3). */
  max?: number;
  /** Extra room above the bottom edge, in px: the height of the selection bar when one shows. */
  inset?: number;
  /** The region's name; a page with several stacks gives each its own. */
  label?: string;
  /** `fixed` over the window (the app), `absolute` inside a positioned container (the gallery). */
  position?: "fixed" | "absolute";
  className?: string;
}

/**
 * The toast region at the bottom right: at most three toasts, newest lowest, 8px apart, 16px from
 * the edges and lifted by `inset` so it sits above the selection bar. Only the toasts take the
 * pointer; the rest of the region lets it through.
 */
export function ToastStack({
  children,
  max = TOAST_STACK_MAX,
  inset = 0,
  position = "fixed",
  label,
  className,
}: ToastStackProps) {
  const toasts = Children.toArray(children).slice(-max);
  return (
    <div
      role="region"
      aria-label={label ?? t("toast.region")}
      style={{ bottom: `calc(var(--space-4) + ${inset}px)` }}
      className={cx(
        "pointer-events-none right-4 z-40 flex flex-col items-end gap-2 [&>*]:pointer-events-auto",
        position === "fixed" ? "fixed" : "absolute",
        className,
      )}
    >
      {toasts}
    </div>
  );
}
