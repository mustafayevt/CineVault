import { Outlet, useRouterState } from "@tanstack/react-router";
import { MotionConfig } from "motion/react";
import { useMemo } from "react";

import { WhatsNewCard } from "../changelog/WhatsNewCard";
import { useWhatsNew } from "../changelog/useWhatsNew";
import { DeleteConfirmDialog } from "../delete/DeleteConfirmDialog";
import { PermanentDeleteDialog } from "../delete/PermanentDeleteDialog";
import { JobAnnouncer } from "../jobs/JobAnnouncer";
import { useJobProgressStream } from "../jobs/useJobProgressStream";
import { useItemsChangedSync } from "../live/useItemsChanged";
import { useBackendNotices } from "../notifications/useBackendNotices";
import { useRefreshQueueAfterMatch } from "../scrape/useRefreshQueue";
import {
  useRefreshAfterJobs,
  useRefreshDuringScan,
  useSidebarSources,
} from "../scan/useSourceHealth";
import { useAppearance, useSettingsLoader } from "../settings/appearance";
import type { MotionPreference } from "../settings/appearance";
import { MoveToHost } from "../move-to/MoveToMenu";
import { useLocaleSettings } from "../settings/localeSettings";
import { useSetting } from "../settings/settingsStore";
import { ToastViewport } from "../toasts/ToastViewport";
import { useUndoShortcut } from "../toasts/useUndoShortcut";
import { useUpdateChecks } from "../updates/useUpdateChecks";
import { BACKDROP_SLOT_ID } from "../title/backdropSlot";
import { useDesktopGuards } from "./desktopGuards";
import { detectPlatform } from "./platform";
import type { Platform } from "./platform";
import { SidebarPanel } from "./SidebarPanel";
import { TitleBar } from "./TitleBar";
import { useNavigationHistory } from "./useNavigationHistory";
import { useShellDemo } from "./shellDemo";
import { usePageScrollReset } from "./usePageScrollReset";
import { ROUTE_CONTENT_ATTRIBUTE, useRouteFocus } from "./useRouteFocus";
import { tauriWindowApi } from "./windowApi";
import type { WindowApi } from "./windowApi";

const defaultPlatform = detectPlatform();

/** Maps the Motion setting onto the animation library's own switch. */
const MOTION_CONFIG: Readonly<Record<MotionPreference, "user" | "always" | "never">> = {
  system: "user",
  reduced: "always",
  full: "never",
};

interface AppShellProps {
  platform?: Platform;
  /** The window to control; defaults to the current Tauri window (none in a plain browser). */
  windowApi?: WindowApi | null;
}

/**
 * The window frame of the desktop design: the sidebar panel floats on the window
 * ground with an equal 8px gutter above, left of and below it and 8px to the content; the title
 * bar and the page sit flat on the ground in the column to its right. Pages cross-fade
 * (`.cv-page`) when the address changes page, and toasts stack at the bottom right.
 */
export function AppShell({ platform = defaultPlatform, windowApi }: AppShellProps) {
  useJobProgressStream();
  useBackendNotices();
  useSettingsLoader();
  useAppearance();
  useLocaleSettings();
  // Pages refresh in place when a write changes a title they show.
  useItemsChangedSync();
  useRefreshQueueAfterMatch();
  useSidebarSources();
  useRefreshAfterJobs();
  useRefreshDuringScan();
  useUndoShortcut(platform);
  useUpdateChecks();
  useWhatsNew();
  useRouteFocus();
  const history = useNavigationHistory();
  useDesktopGuards({ back: history.back, forward: history.forward });
  useShellDemo();
  const main = usePageScrollReset<HTMLElement>();
  const motion = useSetting<MotionPreference>("appearance.motion");
  const api = useMemo(() => (windowApi === undefined ? tauriWindowApi() : windowApi), [windowApi]);
  const pathname = useRouterState({ select: (state) => state.location.pathname });
  return (
    <MotionConfig reducedMotion={MOTION_CONFIG[motion]}>
      <div
        data-platform={platform}
        data-testid="app-frame"
        className="relative flex h-full bg-surface-window text-text"
      >
        {/* The layer title pages draw their hero backdrop into: it bleeds under the panel and the
            title bar, and the panel and the column paint above it. */}
        <div
          id={BACKDROP_SLOT_ID}
          aria-hidden="true"
          className="pointer-events-none absolute inset-0 overflow-hidden"
        />
        <div className="relative flex shrink-0 p-(--gutter)">
          <SidebarPanel platform={platform} />
        </div>
        <div className="relative flex min-w-0 flex-1 flex-col">
          <TitleBar platform={platform} windowApi={api} />
          {/* The one main landmark: every page renders inside it, flat on the window ground.
              It is the containing block for positioned content, so nothing inside a page can
              stretch the window past its height. */}
          <main
            ref={main}
            {...{ [ROUTE_CONTENT_ATTRIBUTE]: "" }}
            className="@container-size relative flex min-h-0 flex-1 flex-col overflow-auto [container-name:pane] [scrollbar-gutter:stable]"
          >
            {/* Keyed by page, not by search: a new page fades in, a filter change does not. */}
            <div key={pathname} className="cv-page flex min-h-0 flex-1 flex-col">
              <Outlet />
            </div>
          </main>
        </div>
        <MoveToHost />
        <DeleteConfirmDialog />
        <PermanentDeleteDialog />
        <WhatsNewCard />
        <ToastViewport platform={platform} />
        <JobAnnouncer />
      </div>
    </MotionConfig>
  );
}
