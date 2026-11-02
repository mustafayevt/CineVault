import { ChevronDown, Ellipsis, X } from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useState } from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { Button } from "./Button";
import { Icon } from "./Icon";
import { IconButton } from "./IconButton";
import { Menu } from "./Menu";
import { MenuItem } from "./MenuItem";
import { Presence } from "./Presence";

/** One action of the bar or of its More menu; the list comes from the one title-actions list. */
export interface SelectionBarAction {
  id: string;
  label: string;
  icon?: LucideIcon;
  onSelect: () => void;
  /** Key hint, shown in the More menu. */
  shortcut?: string;
  /** A destructive action: the danger colour. */
  danger?: boolean;
  disabled?: boolean;
  /** Keeps the action focusable and says why it cannot run. */
  disabledReason?: string;
  enabledBy?: string;
}

interface SelectionBarProps {
  /** How many items are selected; the bar is shown while it is above zero. */
  count: number;
  /** The total size of the selection, already formatted ("186.2 GB"). */
  size?: string;
  /** The actions shown on the bar, in order. */
  actions: readonly SelectionBarAction[];
  /** The actions behind More; the button is left out when there are none. */
  more?: readonly SelectionBarAction[];
  /** Clears the selection (the X button; Esc does the same in the list). */
  onClear: () => void;
  className?: string;
}

/** On a phone an action with an icon is the icon alone, in a square as tall as the button. */
const PHONE_ICON_ONLY =
  "@max-[640px]/pane:w-(--h-btn) @max-[640px]/pane:gap-0 @max-[640px]/pane:px-0";

function Divider({ className }: { className?: string }) {
  return <span aria-hidden="true" className={cx("h-[18px] w-px bg-line-section", className)} />;
}

/**
 * The floating selection bar: a toolbar centred 18px above the bottom of the
 * list, on the floating surface at radius 14. It slides up and fades in (`.cv-slide-up`, 180 ms)
 * and out again when the selection empties, and shows the count, the total size, the actions and
 * a More menu. Put it inside a positioned container; it ignores the pointer outside the bar. The
 * list it covers keeps 16px of room under its last row.
 */
export function SelectionBar({
  count,
  size,
  actions,
  more = [],
  onClear,
  className,
}: SelectionBarProps) {
  // The count and size shown while the bar leaves, after the selection has already emptied.
  const [shown, setShown] = useState({ count, size });
  if (count > 0 && (shown.count !== count || shown.size !== size)) setShown({ count, size });
  const present = count > 0;
  return (
    <Presence present={present}>
      {(presence) => (
        <div
          className={cx(
            "pointer-events-none absolute inset-x-0 bottom-[18px] z-30 flex justify-center px-4",
            className,
          )}
        >
          <div
            {...presence}
            role="toolbar"
            aria-label={t("selection.bar")}
            className={cx(
              "cv-slide-up pointer-events-auto flex max-w-full min-w-0 items-center gap-1 rounded-(--r-14) border border-solid",
              "border-line-edge bg-surface-floating py-1.5 pr-1.5 pl-3.5 text-(length:--fs-13)",
              "whitespace-nowrap text-text shadow-floating",
            )}
          >
            <span aria-live="polite" className="shrink-0 font-(weight:--fw-600)">
              {t("selection.count", { count: shown.count })}
            </span>
            {shown.size !== undefined && (
              <span className="mr-2.5 ml-1 text-text-muted tabular-nums @max-[640px]/pane:hidden">
                {shown.size}
              </span>
            )}
            <Divider className="mr-1 shrink-0" />
            {/* On a phone the actions are icons (named for screen readers) and scroll sideways if
                they still do not fit; the count, More and the clear button stay put. */}
            <div
              className={cx(
                "flex min-w-0 items-center gap-1",
                "@max-[640px]/pane:-my-1 @max-[640px]/pane:overflow-x-auto @max-[640px]/pane:py-1",
                "@max-[640px]/pane:[scrollbar-width:none] @max-[640px]/pane:[&::-webkit-scrollbar]:hidden",
              )}
            >
              {actions.map((action) => (
                <Button
                  key={action.id}
                  variant="ghost"
                  icon={action.icon}
                  disabled={action.disabled}
                  disabledReason={action.disabledReason}
                  enabledBy={action.enabledBy}
                  onClick={action.onSelect}
                  className={cx(
                    action.danger ? "text-danger enabled:hover:text-danger" : "text-text",
                    action.icon !== undefined && PHONE_ICON_ONLY,
                  )}
                >
                  {action.icon === undefined ? (
                    action.label
                  ) : (
                    <span className="@max-[640px]/pane:sr-only">{action.label}</span>
                  )}
                </Button>
              ))}
            </div>
            {more.length > 0 && (
              <>
                <Divider className="mx-1 shrink-0 @max-[640px]/pane:mx-0" />
                <Menu
                  side="top"
                  align="end"
                  trigger={
                    <Button
                      variant="ghost"
                      trailingIcon={ChevronDown}
                      className={cx("text-text @max-[640px]/pane:[&>svg]:hidden", PHONE_ICON_ONLY)}
                    >
                      <Icon icon={Ellipsis} className="hidden @max-[640px]/pane:block" />
                      <span className="@max-[640px]/pane:sr-only">
                        {t("selection.action.more")}
                      </span>
                    </Button>
                  }
                >
                  {more.map((action) => (
                    <MenuItem
                      key={action.id}
                      icon={action.icon}
                      shortcut={action.shortcut}
                      danger={action.danger}
                      disabled={action.disabled}
                      disabledReason={action.disabledReason}
                      enabledBy={action.enabledBy}
                      onSelect={action.onSelect}
                    >
                      {action.label}
                    </MenuItem>
                  ))}
                </Menu>
              </>
            )}
            <IconButton aria-label={t("selection.clear")} icon={X} onClick={onClear} />
          </div>
        </div>
      )}
    </Presence>
  );
}
