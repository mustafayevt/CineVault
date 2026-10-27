import { Check, Minus } from "lucide-react";
import { useEffect, useRef } from "react";
import type { InputHTMLAttributes, ReactNode, Ref } from "react";

import { cx } from "../lib/cx";

interface CheckboxProps extends Omit<
  InputHTMLAttributes<HTMLInputElement>,
  "type" | "onChange" | "checked" | "size"
> {
  checked: boolean;
  /** Some of a group's rows are chosen: draws a dash and reports `mixed`. */
  indeterminate?: boolean;
  onCheckedChange: (checked: boolean) => void;
  /** A visible label after the box. Without one, pass `aria-label`. */
  children?: ReactNode;
  ref?: Ref<HTMLInputElement>;
}

/**
 * A 14px checkbox: radius 4, a 1.5px edge, gold when on with a dark check. It is a real checkbox
 * input, so Space toggles it and a label click works. Name it with children or `aria-label`.
 */
export function Checkbox({
  checked,
  indeterminate = false,
  onCheckedChange,
  children,
  className,
  disabled,
  ref,
  ...rest
}: CheckboxProps) {
  const own = useRef<HTMLInputElement | null>(null);
  useEffect(() => {
    if (own.current) own.current.indeterminate = indeterminate;
  }, [indeterminate]);
  const filled = checked || indeterminate;
  const box = (
    <span
      className={cx(
        // A block box when it stands alone, so the line height of a text parent never pushes it
        // below the centre line of a table row.
        "relative size-3.5 shrink-0 items-center justify-center",
        children === undefined ? "flex" : "inline-flex",
        children === undefined && className,
      )}
    >
      <input
        {...rest}
        ref={(node) => {
          own.current = node;
          if (typeof ref === "function") ref(node);
          else if (ref) ref.current = node;
        }}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        onChange={(event) => onCheckedChange(event.target.checked)}
        className={cx(
          "cv-hoverable absolute inset-0 m-0 size-3.5 appearance-none rounded-(--r-4)",
          "border-[1.5px] border-solid",
          filled ? "border-gold bg-gold" : "border-text-disabled bg-transparent",
          !disabled && !filled && "hover:border-text-muted",
          !disabled && filled && "hover:border-gold-hover hover:bg-gold-hover",
          disabled && "border-line-edge",
          disabled && filled && "bg-text-disabled",
        )}
      />
      <span aria-hidden="true" className="pointer-events-none relative text-on-gold">
        {indeterminate ? (
          <Minus size={10} strokeWidth={3} />
        ) : (
          checked && <Check size={10} strokeWidth={3} />
        )}
      </span>
    </span>
  );
  if (children === undefined) return box;
  return (
    <label
      className={cx(
        "type-body inline-flex items-center gap-2 text-text",
        disabled && "text-text-disabled",
        className,
      )}
    >
      {box}
      {children}
    </label>
  );
}
