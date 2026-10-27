import { ListFilter, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import type { ComponentProps, ReactNode } from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { Icon } from "./Icon";

interface FilterChipProps {
  /** Field name, shown muted: "Year". */
  field: string;
  /** Value, shown in the text colour: "2010..2019". */
  value: string;
  /** A negated chip reads "not <value>" on a `danger-soft` ground. */
  negated?: boolean;
  /** The remove button's accessible name; defaults to "Remove filter <field> <value>". */
  removeLabel?: string;
  onRemove?: () => void;
  /** Clicking the chip edits it. With it (or `onNegate`) the chip's text is a button. */
  onEdit?: () => void;
  /** Alt+click (or Alt+Enter) turns the chip into its opposite: "watched" into "not watched". */
  onNegate?: () => void;
  /** The edit button's accessible name; defaults to "Edit filter <field> <value>". */
  editLabel?: string;
  className?: string;
}

/**
 * A removable tonal tag (3px radius) showing one active library filter; its value is in the accent. A click on its text edits it, Alt+click
 * negates it; a negated chip has a bold "not" and its value struck through.
 */
export function FilterChip({
  field,
  value,
  negated = false,
  removeLabel,
  onRemove,
  onEdit,
  onNegate,
  editLabel,
  className,
}: FilterChipProps) {
  const text = (
    <>
      {field !== "" && <span className={negated ? undefined : "text-text-muted"}>{field}</span>}
      {negated && <b className="font-bold">{t("components.negated")}</b>}
      {negated ? (
        <s className="line-through">{value}</s>
      ) : (
        <span className="text-gold">{value}</span>
      )}
    </>
  );
  const interactive = onEdit !== undefined || onNegate !== undefined;
  return (
    <span
      className={cx(
        "type-body inline-flex h-7 items-center gap-1 rounded-(--r-4) pr-1",
        interactive ? "pl-1" : "pl-3",
        negated ? "bg-surface-active text-danger" : "bg-surface-card text-text",
        className,
      )}
    >
      {interactive ? (
        <button
          type="button"
          aria-label={
            editLabel ??
            t("components.editFilter", {
              chip: [field, negated ? t("components.negated") : "", value]
                .filter(Boolean)
                .join(" "),
            })
          }
          title={onNegate === undefined ? undefined : t("components.editFilter.hint")}
          onClick={(event) => {
            if (event.altKey && onNegate !== undefined) onNegate();
            else onEdit?.();
          }}
          onKeyDown={(event) => {
            if (event.key === "Enter" && event.altKey && onNegate !== undefined) {
              event.preventDefault();
              onNegate();
            }
          }}
          className={cx(
            "inline-flex h-6 items-center gap-1 rounded-(--r-4) px-2",
            "transition-colors duration-(--dur-hover) ease-(--ease-out) hover:bg-surface-raised",
          )}
        >
          {text}
        </button>
      ) : (
        text
      )}
      <button
        type="button"
        aria-label={removeLabel ?? t("components.removeFilter", { field, value })}
        onClick={onRemove}
        className={cx(
          "inline-flex size-5 items-center justify-center rounded-(--r-4) text-text-muted",
          "transition-colors duration-(--dur-hover) ease-(--ease-out) hover:bg-surface-raised hover:text-text",
        )}
      >
        <Icon icon={X} size={14} />
      </button>
    </span>
  );
}

/** The quiet "Filter" verb that opens the facet picker: a ghost button, never a dashed pill. */
export function AddFilterChip({
  className,
  children = "Filter",
  type = "button",
  ...rest
}: ComponentProps<"button">) {
  return (
    <button
      type={type}
      className={cx(
        "type-body inline-flex h-(--h-btn-lg) items-center gap-[7px] rounded-(--r-6) px-3",
        "text-text-2 transition-colors duration-(--dur-hover) ease-(--ease-out) hover:bg-surface-raised hover:text-text",
        className,
      )}
      {...rest}
    >
      <Icon icon={ListFilter} size={16} className="size-[15px]" />
      {children}
    </button>
  );
}

interface ChipProps extends Omit<ComponentProps<"button">, "children"> {
  /** On: the gold tint with a gold edge. A chip with `active` set is a toggle (`aria-pressed`). */
  active?: boolean;
  /** `warn` for a chip that counts problems; it keeps the warning colour, on or off. */
  tone?: "default" | "warn";
  /** Leading glyph, drawn at 12px. */
  icon?: LucideIcon;
  /** Trailing glyph, for example a chevron on a chip that opens a menu, drawn at 12px. */
  trailingIcon?: LucideIcon;
  /** A count after the label, 600 and tabular. */
  count?: number | string;
  children: ReactNode;
}

/**
 * A filter or toggle chip: 24px, a pill with a `--line-edge` border and 12px text; on, it takes the
 * gold tint and gold edge. Chips and 28px toolbar buttons on one row share a centre line.
 */
export function Chip({
  active,
  tone = "default",
  icon,
  trailingIcon,
  count,
  className,
  type = "button",
  children,
  ...rest
}: ChipProps) {
  return (
    <button
      type={type}
      aria-pressed={active}
      className={cx(
        "cv-hoverable inline-flex h-(--h-chip) shrink-0 items-center gap-[5px] rounded-(--r-pill)",
        "border border-solid px-2.5 text-(length:--fs-12) leading-none font-(weight:--fw-500) whitespace-nowrap",
        "[&_svg]:size-3 [&_svg]:shrink-0",
        active
          ? "border-gold-tint-edge bg-gold-tint text-gold"
          : cx(
              "bg-transparent",
              tone === "warn"
                ? "border-warn-bar text-warn"
                : "border-line-edge text-text-2 enabled:hover:bg-surface-active enabled:hover:text-text",
              "enabled:active:bg-surface-pressed",
            ),
        "disabled:text-text-disabled",
        className,
      )}
      {...rest}
    >
      {icon && <Icon icon={icon} size={12} />}
      {children}
      {count !== undefined && (
        <b
          className={cx("font-(weight:--fw-600) tabular-nums", active ? "text-gold" : "text-text")}
        >
          {count}
        </b>
      )}
      {trailingIcon && <Icon icon={trailingIcon} size={12} />}
    </button>
  );
}
