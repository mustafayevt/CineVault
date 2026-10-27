import { Link } from "@tanstack/react-router";

import { SectionLabel } from "../../components/SectionLabel";
import { SpaceBar } from "../../components/SpaceBar";
import { Tooltip } from "../../components/Tooltip";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import type { SidebarDrive } from "./useSidebarDrives";

interface DriveMetersProps {
  drives: readonly SidebarDrive[];
  collapsed: boolean;
}

/**
 * The Drives section of the sidebar (spec `drives` "Drives in the sidebar"):
 * each drive's name, its free space and a thin used-space bar, warning-coloured when the drive is
 * low and greyed with "Offline" when it is gone. Hovering shows "<name> · <free> free of <total>";
 * choosing a drive opens the Drives page. Collapsed, each drive is a small meter.
 */
export function DriveMeters({ drives, collapsed }: DriveMetersProps) {
  const format = useFormat();
  if (drives.length === 0) return null;
  return (
    <section aria-labelledby="sidebar-drives" className="flex flex-col">
      {collapsed ? (
        <h2 id="sidebar-drives" className="sr-only">
          {t("shell.page.drives")}
        </h2>
      ) : (
        <SectionLabel as="h2" id="sidebar-drives" className="mb-0 px-2.5 pt-[18px] pb-1.5">
          {t("shell.page.drives")}
        </SectionLabel>
      )}
      <ul className={cx("m-0 flex list-none flex-col gap-0.5 p-0", collapsed && "pt-3")}>
        {drives.map((drive) => {
          const free = format.size(drive.freeBytes);
          const tip = drive.online
            ? t("shell.drives.tip", {
                name: drive.name,
                free,
                total: format.size(drive.totalBytes),
              })
            : t("shell.drives.offlineTip", { name: drive.name });
          const note = drive.online ? t("shell.drives.free", { free }) : t("shell.drives.offline");
          const used = drive.totalBytes > 0 ? 1 - drive.freeBytes / drive.totalBytes : 0;
          const state = !drive.online ? "offline" : drive.low ? "low" : "normal";
          return (
            <li key={drive.id}>
              <Tooltip content={tip} side={collapsed ? "right" : "top"}>
                <Link
                  to="/drives"
                  aria-label={collapsed ? tip : undefined}
                  data-drive={drive.id}
                  data-state={state}
                  className={cx(
                    "cv-hoverable flex flex-col gap-1.5 rounded-(--r-6) py-[7px] no-underline",
                    "hover:bg-surface-floating-hover active:bg-surface-pressed",
                    collapsed ? "items-center px-1.5" : "px-2.5",
                    drive.online ? "text-text" : "text-text-disabled",
                  )}
                >
                  {collapsed ? null : (
                    <span className="flex justify-between gap-2 text-(length:--fs-12) whitespace-nowrap">
                      <span className="min-w-0 truncate font-(weight:--fw-600)">{drive.name}</span>
                      <span
                        className={
                          !drive.online
                            ? "text-text-disabled"
                            : drive.low
                              ? "text-warn"
                              : "text-text-muted"
                        }
                      >
                        {note}
                      </span>
                    </span>
                  )}
                  <SpaceBar used={used} label={tip} state={state} />
                </Link>
              </Tooltip>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
