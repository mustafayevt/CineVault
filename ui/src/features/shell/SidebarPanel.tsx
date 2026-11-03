import { Link, useRouterState } from "@tanstack/react-router";
import {
  ChevronLeft,
  ChevronRight,
  Download,
  HardDrive,
  History,
  LayoutGrid,
  Moon,
  PanelLeftClose,
  PanelLeftOpen,
  PenLine,
  ScanSearch,
  Settings,
  Sun,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { BrandMark } from "../../components/BrandMark";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { NavItem } from "../../components/NavItem";
import { Tooltip } from "../../components/Tooltip";
import { buttonBase, buttonLooks, iconButtonSizes } from "../../components/buttonStyles";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { selectSidebarCollapsed, useShellStore } from "../../stores/shell";
import type { Platform } from "./platform";
import { useMetadataOnly } from "../settings/metadataOnly";
import { useBackForwardInput } from "./backForward";
import { destinationOf } from "./destinations";
import type { Destination } from "./destinations";
import { DriveMeters } from "./DriveMeters";
import { useNavCounts } from "./navCounts";
import { SidebarProgress } from "./SidebarProgress";
import { useNavigationHistory } from "./useNavigationHistory";
import { useNeedsAttention } from "../activity/attentionModel";
import { useSidebarDrives } from "./useSidebarDrives";

interface NavTarget {
  id: Exclude<Destination, "activity" | "settings">;
  to: "/library" | "/scrape" | "/rename" | "/drives" | "/torrents";
  label: MessageKey;
  icon: LucideIcon;
}

const NAV: readonly NavTarget[] = [
  { id: "library", to: "/library", label: "shell.page.library", icon: LayoutGrid },
  { id: "scrape", to: "/scrape", label: "shell.page.scrape", icon: ScanSearch },
  { id: "rename", to: "/rename", label: "shell.page.rename", icon: PenLine },
  { id: "drives", to: "/drives", label: "shell.page.drives", icon: HardDrive },
  { id: "torrents", to: "/torrents", label: "shell.page.torrents", icon: Download },
];

/** A square 28px control of the bottom icon row with an 18px icon. */
const rowIconBase = cx(buttonBase, iconButtonSizes.md, "[&_svg]:size-[18px]");
const rowIcon = cx(rowIconBase, buttonLooks.ghost);
/** The active row icon leaves the ghost look out: `cx` does not merge, so both fills would fight. */
const rowIconActive = cx(rowIconBase, "bg-surface-active text-gold hover:text-gold");

interface RowLinkProps {
  to: "/activity" | "/settings";
  label: string;
  icon: LucideIcon;
  active: boolean;
  side: "top" | "right";
  /** The accessible name when it says more than the tooltip ("Activity, 2 things need attention"). */
  name?: string;
  /** A warning dot on the icon: something waits for the user there. */
  warn?: boolean;
}

function RowLink({ to, label, icon, active, side, name, warn = false }: RowLinkProps) {
  return (
    <Tooltip content={name ?? label} side={side}>
      <Link
        to={to}
        aria-label={name ?? label}
        aria-current={active ? "page" : undefined}
        data-warn={warn ? "true" : undefined}
        className={cx(active ? rowIconActive : rowIcon, "relative")}
      >
        <Icon icon={icon} size={18} />
        {warn ? (
          <span
            aria-hidden="true"
            className="absolute top-1 right-1 size-2 rounded-full bg-warn-bar ring-2 ring-surface-floating"
          />
        ) : null}
      </Link>
    </Tooltip>
  );
}

interface SidebarPanelProps {
  platform: Platform;
  /**
   * `panel` (default) floats at the left of the window; `drawer` is the same panel inside the
   * compact layout's drawer: always full, wider, without back and forward (they are in the title
   * bar there) and without the collapse toggle.
   */
  variant?: "panel" | "drawer";
}

/**
 * The sidebar: a full-height panel floating on the window ground, 200px wide inside the 8px gutter with the
 * floating-surface recipe. From the top: the logo row with back and forward (lined up with the nav
 * icons; on macOS the traffic lights sit in it too), the five labelled destinations with their
 * waiting counts, the Drives section, and at the foot the progress card above the icon row
 * Activity (with a warning dot while something needs attention) · Theme · Settings · Collapse. Collapsed, it is a column of icons with tooltips, small
 * drive meters and a progress ring; the choice is remembered.
 */
export function SidebarPanel({ platform, variant = "panel" }: SidebarPanelProps) {
  // In the compact layout the title bar holds back and forward.
  useBackForwardInput(variant === "panel");
  const nav = useNavigationHistory();
  const counts = useNavCounts();
  const drives = useSidebarDrives();
  const drawer = variant === "drawer";
  const railChoice = useShellStore(selectSidebarCollapsed);
  const collapsed = !drawer && railChoice;
  const toggleSidebar = useShellStore((state) => state.toggleSidebar);
  const theme = useShellStore((state) => state.theme);
  const toggleTheme = useShellStore((state) => state.toggleTheme);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  const active = destinationOf(pathname);
  // The traffic lights sit in the panel's top row only in the desktop window.
  const mac = platform === "macos" && !drawer;
  const lowDrives = drives.filter((drive) => drive.online && drive.low).length;
  const attention = useNeedsAttention().length;
  // "Metadata only" renames nothing, so Rename has no page to offer.
  const metadataOnly = useMetadataOnly();
  const targets = metadataOnly ? NAV.filter((target) => target.id !== "rename") : NAV;
  const tipSide = collapsed ? "right" : "top";

  const countOf = (id: NavTarget["id"]): { count?: number | string; tone?: "warn" } => {
    if (id === "scrape" && counts.scrape > 0) return { count: counts.scrape };
    if (id === "rename" && counts.rename > 0) return { count: counts.rename };
    if (id === "torrents" && counts.torrents > 0) return { count: counts.torrents };
    if (id === "drives" && lowDrives > 0) {
      return { count: t("shell.drives.low", { count: lowDrives }), tone: "warn" };
    }
    return {};
  };

  return (
    <nav
      aria-label={t("shell.sidebar")}
      data-testid="sidebar-panel"
      data-collapsed={collapsed ? "true" : "false"}
      data-variant={variant}
      className={cx(
        "flex h-full flex-col overflow-x-hidden overflow-y-auto px-2 pb-2 select-none",
        "rounded-(--r-14) border border-solid border-line-edge bg-surface-floating shadow-floating",
        drawer ? "w-[272px] max-w-full" : collapsed ? (mac ? "w-[76px]" : "w-[50px]") : "w-[200px]",
      )}
    >
      <div
        data-tauri-drag-region
        data-testid="sidebar-top-row"
        className={cx(
          "flex h-[37px] shrink-0 items-center gap-2",
          collapsed ? "justify-center" : mac ? "pl-16" : "pl-2.5",
        )}
      >
        {collapsed && mac ? null : <BrandMark variant="symbol" size={18} />}
        {collapsed ? null : (
          <>
            <span
              data-tauri-drag-region
              className={cx(
                "grow text-(length:--fs-13) font-(weight:--fw-600) text-text",
                mac && "sr-only",
              )}
            >
              CineVault
            </span>
            {mac ? <span data-tauri-drag-region className="grow" /> : null}
            {drawer ? null : (
              <>
                <IconButton
                  icon={ChevronLeft}
                  aria-label={t("chrome.back")}
                  title={t("shell.backHint")}
                  onClick={nav.back}
                  disabled={!nav.canGoBack}
                />
                <IconButton
                  icon={ChevronRight}
                  aria-label={t("chrome.forward")}
                  title={t("shell.forwardHint")}
                  onClick={nav.forward}
                  disabled={!nav.canGoForward}
                />
              </>
            )}
          </>
        )}
      </div>

      <ul className={cx("m-0 flex list-none flex-col gap-0.5 p-0", collapsed && "items-center")}>
        {targets.map((target) => {
          const { count, tone } = countOf(target.id);
          return (
            <li key={target.id} className={collapsed ? undefined : "w-full"}>
              <NavItem
                as={Link}
                to={target.to}
                icon={target.icon}
                label={t(target.label)}
                active={active === target.id}
                count={count}
                countTone={tone}
                collapsed={collapsed}
              />
            </li>
          );
        })}
      </ul>

      <DriveMeters drives={drives} collapsed={collapsed} />

      <div className="min-h-3 grow" />

      <div className="flex flex-col gap-3">
        <SidebarProgress collapsed={collapsed} />
        <div
          className={cx(
            "flex border-t border-solid border-line-section pt-2",
            collapsed ? "flex-col items-center gap-0.5" : "justify-around",
          )}
        >
          <RowLink
            to="/activity"
            label={t("activity.nav")}
            icon={History}
            active={active === "activity"}
            side={tipSide}
            warn={attention > 0}
            {...(attention > 0 ? { name: t("activity.nav.attention", { count: attention }) } : {})}
          />
          <Tooltip
            content={theme === "dark" ? t("shell.theme.toLight") : t("shell.theme.toDark")}
            side={tipSide}
          >
            <button
              type="button"
              aria-label={theme === "dark" ? t("shell.theme.toLight") : t("shell.theme.toDark")}
              className={rowIcon}
              onClick={toggleTheme}
            >
              <Icon icon={theme === "dark" ? Moon : Sun} size={18} />
            </button>
          </Tooltip>
          <RowLink
            to="/settings"
            label={t("shell.page.settings")}
            icon={Settings}
            active={active === "settings"}
            side={tipSide}
          />
          {drawer ? null : (
            <Tooltip content={collapsed ? t("shell.expand") : t("shell.collapse")} side={tipSide}>
              <button
                type="button"
                aria-label={collapsed ? t("shell.expand") : t("shell.collapse")}
                aria-expanded={!collapsed}
                className={rowIcon}
                onClick={toggleSidebar}
              >
                <Icon icon={collapsed ? PanelLeftOpen : PanelLeftClose} size={18} />
              </button>
            </Tooltip>
          )}
        </div>
      </div>
    </nav>
  );
}
