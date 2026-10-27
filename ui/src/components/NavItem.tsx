import type { LucideIcon } from "lucide-react";
import type { ComponentPropsWithoutRef, ElementType, ReactNode } from "react";

import { cx } from "../lib/cx";
import { Icon } from "./Icon";
import { Tooltip } from "./Tooltip";

type NavItemOwnProps<T extends ElementType> = {
  /** What to render: a `button` (default), an `a`, or a router link such as `NavLink`. */
  as?: T;
  icon: LucideIcon;
  label: string;
  /** The page being shown: active fill, gold icon, 600 weight and `aria-current="page"`. */
  active?: boolean;
  /** A count after the label (the queue length, "1 low"). */
  count?: number | string;
  /** `warn` colours the count for something that needs attention. */
  countTone?: "default" | "warn";
  /** The icon-only form for the collapsed sidebar: a 32px square with the label as a tooltip. */
  collapsed?: boolean;
  className?: string;
  children?: ReactNode;
};

type NavItemProps<T extends ElementType> = NavItemOwnProps<T> &
  Omit<ComponentPropsWithoutRef<T>, keyof NavItemOwnProps<T>>;

const base =
  "cv-hoverable relative flex h-(--h-nav) items-center rounded-(--r-6) text-(length:--fs-13) " +
  "leading-none whitespace-nowrap no-underline [&_svg]:shrink-0";

/**
 * A navigation row of the sidebar: 32px, padding 0 10, radius 6, 13/500, an
 * 18px icon and an optional count pill. The active page takes the active rung, 600 and a gold
 * icon; hover and pressed are the next rungs. Collapsed, it is an icon only and the label moves
 * to a tooltip (and stays the accessible name, with the count after it).
 */
export function NavItem<T extends ElementType = "button">({
  as,
  icon,
  label,
  active = false,
  count,
  countTone = "default",
  collapsed = false,
  className,
  children,
  ...rest
}: NavItemProps<T>) {
  const Component: ElementType = as ?? "button";
  const hasCount = count !== undefined && count !== "";
  const spoken = hasCount ? `${label}, ${count}` : label;
  // The tooltip of the collapsed item reads "Scrape · 12" (spec app-shell).
  const shown = hasCount ? `${label} · ${count}` : label;
  const control = (
    <Component
      {...(Component === "button" ? { type: "button" } : {})}
      {...rest}
      aria-current={active ? "page" : undefined}
      // With a count the name reads "Scrape, 12" rather than the run-together "Scrape12".
      aria-label={collapsed || hasCount ? spoken : undefined}
      data-active={active ? "true" : undefined}
      className={cx(
        base,
        collapsed ? "w-(--h-nav) justify-center" : "w-full gap-2.5 px-2.5",
        active
          ? "bg-surface-active font-(weight:--fw-600) text-text"
          : cx(
              "font-(weight:--fw-500) text-text-2",
              "hover:bg-surface-floating-hover hover:text-text active:bg-surface-pressed",
            ),
        className,
      )}
    >
      <Icon icon={icon} size={18} className={active ? "text-gold" : undefined} />
      {collapsed ? (
        hasCount && (
          <span
            aria-hidden="true"
            data-testid="nav-count-dot"
            className={cx(
              "absolute top-1.5 right-1.5 size-1.5 rounded-(--r-pill)",
              countTone === "warn" ? "bg-warn-bar" : "bg-gold",
            )}
          />
        )
      ) : (
        <>
          <span className="min-w-0 grow truncate text-left">{label}</span>
          {hasCount && (
            <span
              data-tone={countTone}
              className={cx(
                "inline-flex h-(--h-tag) items-center rounded-(--r-4) px-1.5 text-(length:--fs-11)",
                "font-(weight:--fw-600) tabular-nums",
                countTone === "warn"
                  ? "bg-warn-tint text-warn"
                  : active
                    ? "bg-surface-pressed text-text-2"
                    : "bg-surface-active text-text-2",
              )}
            >
              {count}
            </span>
          )}
        </>
      )}
      {children}
    </Component>
  );
  return collapsed ? (
    <Tooltip content={shown} side="right">
      {control}
    </Tooltip>
  ) : (
    control
  );
}
