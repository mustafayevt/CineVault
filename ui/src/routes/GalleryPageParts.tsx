import type { ReactNode } from "react";

import { cx } from "../lib/cx";

/** A titled group of the gallery: one component family and all its states. */
export function Group({
  title,
  note,
  children,
}: {
  title: string;
  note?: string;
  children: ReactNode;
}) {
  return (
    <div data-group={title} className="flex flex-col gap-3">
      <div className="flex flex-col gap-0.5 border-b border-solid border-line-section pb-1.5">
        <h3 className="m-0 text-(length:--fs-14) font-(weight:--fw-600) text-text">{title}</h3>
        {note !== undefined && <p className="m-0 text-(length:--fs-12) text-text-muted">{note}</p>}
      </div>
      <div className="flex flex-col gap-4">{children}</div>
    </div>
  );
}

/** One row of specimens with a small caption above it. */
export function Row({
  label,
  children,
  className,
}: {
  label: string;
  children: ReactNode;
  className?: string;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <span className="text-(length:--fs-11) font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase">
        {label}
      </span>
      <div className={cx("flex flex-wrap items-center gap-2", className)}>{children}</div>
    </div>
  );
}

/** A framed stage for things that sit on a surface of their own (a sidebar, a list). */
export function Stage({
  children,
  className,
  tone = "window",
}: {
  children: ReactNode;
  className?: string;
  tone?: "window" | "card";
}) {
  return (
    <div
      className={cx(
        "relative rounded-(--r-10) border border-solid border-line-section",
        tone === "window" ? "bg-surface-window" : "bg-surface-card",
        className,
      )}
    >
      {children}
    </div>
  );
}
