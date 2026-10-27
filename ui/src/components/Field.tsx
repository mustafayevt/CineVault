import type { LucideIcon } from "lucide-react";
import { useId } from "react";
import type { InputHTMLAttributes, ReactNode, Ref } from "react";

import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { Kbd } from "./Kbd";
import { useSurface } from "./surface";

export interface FieldProps extends Omit<InputHTMLAttributes<HTMLInputElement>, "size"> {
  /** Visible label above the field. Without one, pass `aria-label`. */
  label?: string;
  /** Helper text below the field. */
  description?: string;
  /** Error text below the field. It says what happened and what to do. */
  error?: string;
  /** Leading icon, drawn at 16px. */
  icon?: LucideIcon;
  /** Keyboard hint shown at the right edge, for example `Ctrl K`. */
  shortcut?: string;
  /** Mono text, for paths and template tokens. */
  mono?: boolean;
  /**
   * The field sits on a floating surface: it uses the well colour instead of the card colour. Left
   * out, it follows the surrounding `SurfaceContext`.
   */
  floating?: boolean;
  /** Content at the right edge inside the field (a clear button). */
  trailing?: ReactNode;
  /** Wrapper class; the box can be styled with `boxClassName`. */
  boxClassName?: string;
  /** The box height token; `Field` is 30px, `SearchField` 34px. */
  height?: "field" | "search";
  ref?: Ref<HTMLInputElement>;
}

/**
 * Single-line text field, 30px: card colour, a `--line-edge` border, radius 6, 13px text. Focus
 * turns the border gold (base rule `[data-field]:focus-within`), with no halo. On a floating
 * surface it uses the well colour.
 */
export function Field({
  label,
  description,
  error,
  icon,
  shortcut,
  mono = false,
  floating,
  trailing,
  boxClassName,
  height = "field",
  className,
  id,
  ref,
  "aria-describedby": ariaDescribedBy,
  ...rest
}: FieldProps) {
  const autoId = useId();
  const inputId = id ?? autoId;
  const surface = useSurface(floating);
  const descriptionId = description && !error ? `${inputId}-description` : undefined;
  const errorId = error ? `${inputId}-error` : undefined;
  const describedBy = [ariaDescribedBy, descriptionId, errorId].filter(Boolean).join(" ");

  return (
    <div className={cx("flex flex-col gap-1", className)}>
      {label && (
        <label htmlFor={inputId} className="type-body-strong text-text-2">
          {label}
        </label>
      )}
      <div
        data-field=""
        data-surface={surface}
        className={cx(
          "cv-hoverable flex items-center gap-2 rounded-(--r-6) border border-solid px-2.5",
          height === "search" ? "h-(--h-search)" : "h-(--h-field)",
          surface === "floating" ? "bg-surface-well" : "bg-surface-card",
          error ? "border-danger" : "border-line-edge",
          rest.disabled && "cursor-not-allowed text-text-disabled",
          boxClassName,
        )}
      >
        {icon && <Icon icon={icon} className="shrink-0 text-text-muted" />}
        <input
          ref={ref}
          id={inputId}
          aria-invalid={error ? true : undefined}
          aria-describedby={describedBy || undefined}
          className={cx(
            "h-full min-w-0 flex-1 bg-transparent text-text outline-none",
            "placeholder:text-text-muted disabled:cursor-not-allowed disabled:text-text-disabled",
            mono ? "type-code" : "type-body",
          )}
          {...rest}
        />
        {trailing}
        {shortcut && <Kbd className="shrink-0">{shortcut}</Kbd>}
      </div>
      {descriptionId && (
        <p id={descriptionId} className="type-small text-text-muted">
          {description}
        </p>
      )}
      {error && (
        <p id={errorId} className="type-small text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
