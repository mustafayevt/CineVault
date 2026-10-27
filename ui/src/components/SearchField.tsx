import { Search, X } from "lucide-react";

import { Field } from "./Field";
import type { FieldProps } from "./Field";
import { IconButton } from "./IconButton";

export interface SearchFieldProps extends Omit<FieldProps, "icon" | "height" | "trailing"> {
  /** Called by the clear button and by Esc while the field has text. Without it neither is offered. */
  onClear?: () => void;
  /** The clear button's accessible name. */
  clearLabel?: string;
}

/**
 * The search field, 34px, with a search glyph. Esc or the clear button empties it when `onClear`
 * is given. Name it with `aria-label`.
 */
export function SearchField({
  onClear,
  clearLabel = "Clear search",
  value,
  onKeyDown,
  ...rest
}: SearchFieldProps) {
  const hasText = typeof value === "string" && value !== "";
  return (
    <Field
      type="text"
      role="searchbox"
      autoComplete="off"
      spellCheck={false}
      {...rest}
      value={value}
      height="search"
      icon={Search}
      trailing={
        onClear !== undefined && hasText ? (
          <IconButton aria-label={clearLabel} icon={X} size="sm" onClick={onClear} />
        ) : undefined
      }
      onKeyDown={(event) => {
        onKeyDown?.(event);
        if (event.key === "Escape" && onClear !== undefined && hasText) {
          event.preventDefault();
          event.stopPropagation();
          onClear();
        }
      }}
    />
  );
}
