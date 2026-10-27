import { ChevronDown } from "lucide-react";
import { useState } from "react";
import type { KeyboardEvent, SelectHTMLAttributes } from "react";

import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { Menu, MenuRadioGroup, MenuRadioItem } from "./Menu";
import { useSurface } from "./surface";

export interface SelectOption {
  value: string;
  label: string;
}

interface SelectProps extends Omit<
  SelectHTMLAttributes<HTMLSelectElement>,
  "onChange" | "value" | "className"
> {
  options: readonly SelectOption[];
  value: string;
  onValueChange: (value: string) => void;
  /** On a floating surface the field takes the well colour; left out it follows `SurfaceContext`. */
  floating?: boolean;
  /** Sizes the whole control (a width, say). */
  className?: string;
}

/** Keys that open the list on a closed field, besides Enter, Space and Down (handled by the menu). */
function opensList(event: KeyboardEvent): boolean {
  return (
    event.key === "F4" ||
    event.key === "ArrowUp" ||
    (event.altKey && (event.key === "ArrowDown" || event.key === "ArrowUp"))
  );
}

/**
 * A drop-down list drawn as a Field (30px, card or well colour, `--line-edge` border, gold border
 * on focus) with a chevron. Clicking it, or Enter, Space, Up, Down, Alt+Down or F4, opens the list
 * as our own Menu (floating surface, 28px items, a check on the chosen one) instead of the
 * platform's popup. The control underneath stays a real `<select>`: it keeps its combobox role,
 * value and options for assistive technology and form tests, and typing a letter on the closed
 * field still picks the next match. Name it with `aria-labelledby` or `aria-label`.
 */
export function Select({
  options,
  value,
  onValueChange,
  floating,
  className,
  disabled,
  onKeyDown,
  onMouseDown,
  ...rest
}: SelectProps) {
  const surface = useSurface(floating);
  const [open, setOpen] = useState(false);
  const [select, setSelect] = useState<HTMLSelectElement | null>(null);

  return (
    <span
      data-field=""
      data-surface={surface}
      className={cx(
        "cv-hoverable relative inline-flex h-(--h-field) min-w-40 items-center rounded-(--r-6)",
        "border border-solid border-line-edge",
        surface === "floating" ? "bg-surface-well" : "bg-surface-card",
        disabled ? "text-text-disabled" : "hover:border-line-kbd",
        className,
      )}
    >
      <Menu
        open={open && disabled !== true}
        onOpenChange={setOpen}
        modal={false}
        style={{ minWidth: "max(264px, var(--radix-dropdown-menu-trigger-width))" }}
        onOpenAutoFocus={(event) => {
          // The list opens on the chosen item, as a native list does.
          const content = event.target instanceof HTMLElement ? event.target : null;
          const chosen = content?.querySelector<HTMLElement>(
            '[role="menuitemradio"][aria-checked="true"]',
          );
          if (chosen === null || chosen === undefined) return;
          event.preventDefault();
          chosen.focus();
        }}
        onCloseAutoFocus={(event) => {
          event.preventDefault();
          select?.focus();
        }}
        trigger={
          <select
            ref={setSelect}
            value={value}
            disabled={disabled}
            onChange={(event) => {
              setOpen(false);
              onValueChange(event.target.value);
            }}
            onMouseDown={(event) => {
              // The list is our menu (opened by the trigger's pointer handler), never the native popup.
              event.preventDefault();
              onMouseDown?.(event);
            }}
            onKeyDown={(event) => {
              onKeyDown?.(event);
              if (!event.defaultPrevented && opensList(event)) {
                event.preventDefault();
                setOpen(true);
              }
            }}
            className={cx(
              "type-body h-full w-full min-w-0 cursor-pointer appearance-none rounded-(--r-6) bg-transparent pr-7 pl-2.5",
              "text-text outline-none disabled:cursor-not-allowed disabled:text-text-disabled",
              "[&>option]:bg-surface-floating [&>option]:text-text",
            )}
            {...rest}
          >
            {options.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        }
      >
        <MenuRadioGroup
          value={value}
          onValueChange={(next) => {
            if (next !== value) onValueChange(next);
          }}
        >
          {options.map((option) => (
            <MenuRadioItem key={option.value} value={option.value}>
              {option.label}
            </MenuRadioItem>
          ))}
        </MenuRadioGroup>
      </Menu>
      <Icon
        icon={ChevronDown}
        className="pointer-events-none absolute right-2 shrink-0 text-text-muted"
      />
    </span>
  );
}
