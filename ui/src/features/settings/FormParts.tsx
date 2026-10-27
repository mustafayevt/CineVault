import type { ReactNode } from "react";

import { cx } from "../../lib/cx";

interface SettingsSectionProps {
  /** The id the index scrolls to. */
  id: string;
  title: string;
  /** The tooltip of the heading: an explanation the page does not print. */
  hint?: string;
  children: ReactNode;
}

/**
 * One section of the Settings page: a 16/700 heading over a card of rows (card colour, hairline
 * edge, radius 10). The heading's id is `<id>-title`.
 */
export function SettingsSection({ id, title, hint, children }: SettingsSectionProps) {
  return (
    <section id={id} aria-labelledby={`${id}-title`} data-settings-section={id}>
      <h2
        id={`${id}-title`}
        title={hint}
        className="m-0 mb-2.5 text-(length:--fs-16) font-(weight:--fw-700)"
      >
        {title}
      </h2>
      <div className="flex flex-col overflow-hidden rounded-(--r-10) border border-solid border-line-section bg-surface-card">
        {children}
      </div>
    </section>
  );
}

interface FormRowProps {
  label?: ReactNode;
  /** The line under the label (12px muted). */
  hint?: ReactNode;
  /** Ties the label to its control. */
  labelId?: string;
  /** Rows with a field and lines under it align to the top. */
  align?: "center" | "start";
  /** Dimmed row (a folder that is offline). */
  dim?: boolean;
  children?: ReactNode;
}

/**
 * A row of a Settings card: the label and its hint in a 200px column, the control beside it. A
 * hairline separates rows.
 */
export function FormRow({ label, hint, labelId, align = "center", dim, children }: FormRowProps) {
  return (
    <div
      className={cx(
        "grid grid-cols-[200px_minmax(0,1fr)] gap-5 border-t border-solid border-line-row px-4 py-3 first:border-t-0",
        align === "start" ? "items-start" : "items-center",
        dim && "text-text-disabled",
      )}
    >
      <div className="min-w-0">
        {label === undefined ? null : (
          <div id={labelId} className="font-(weight:--fw-500)">
            {label}
          </div>
        )}
        {hint === undefined ? null : (
          <div className="mt-px text-(length:--fs-12) text-text-muted">{hint}</div>
        )}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

/**
 * The look of a block that fills a whole row of a Settings card (a card of the torrent sources
 * that holds more than a label and a control): padded like a row, a hairline above.
 */
export const ROW_BLOCK = "flex flex-col gap-3 border-t border-solid border-line-row px-4 py-3";
