import { useNavigate } from "@tanstack/react-router";
import { MoreHorizontal, Trash2 } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { Menu } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { DriveCard } from "./DriveCard";
import { useDrivesOverview } from "./drivesBackend";
import { agoText, offlineCount, trashTotal } from "./drivesModel";
import type { Drive } from "./drivesModel";
import { DriveTitles } from "./DriveTitles";
import { EmptyTrashDialog } from "./EmptyTrashDialog";
import { useNow } from "./useNow";

/** "4 drives · 1 offline". */
function headerText(drives: readonly Drive[]): string {
  const offline = offlineCount(drives);
  return offline === 0
    ? t("drivesPage.header", { count: drives.length })
    : t("drivesPage.header.offline", { count: drives.length, offline });
}

function Empty() {
  const navigate = useNavigate();
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
      <h2 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
        {t("drivesPage.empty.title")}
      </h2>
      <p className="m-0 max-w-[48ch] text-text-2">{t("drivesPage.empty.body")}</p>
      <Button onClick={() => void navigate({ to: "/settings", search: { section: "library" } })}>
        {t("drivesPage.empty.action")}
      </Button>
    </div>
  );
}

/**
 * The Drives page (`/drives`): a card per drive that holds a library folder on the
 * left, and the titles of the chosen drive on the right. The numbers are read when the page opens,
 * when the service says they changed and every minute while the app is in front.
 */
export function DrivesPage() {
  const format = useFormat();
  const overview = useDrivesOverview();
  const now = useNow();
  const [chosen, setChosen] = useState<string | null>(null);
  /** The drive whose Empty… is open (`"*"` for every drive), or `null`. */
  const [emptying, setEmptying] = useState<string | null>(null);

  const drives = overview.data?.drives ?? [];
  // The first drive until the user chooses one, and again if the chosen one went away.
  const drive = drives.find((entry) => entry.id === chosen) ?? drives[0];
  const updatedMs = overview.data?.updatedMs ?? 0;
  const updated =
    updatedMs === 0
      ? ""
      : (() => {
          const when = agoText(updatedMs, now, format.locale);
          return when === "" ? t("drivesPage.updated.now") : t("drivesPage.updated", { when });
        })();

  return (
    <section aria-label={t("drivesPage.region")} className="relative flex h-full min-h-0">
      <h1 className="sr-only">{t("drivesPage.region")}</h1>
      {overview.isPending ? (
        <div
          aria-busy="true"
          className="flex w-[380px] shrink-0 flex-col gap-2.5 border-r border-solid border-line-section px-6 py-4"
        >
          {[0, 1, 2].map((n) => (
            <Skeleton key={n} shape="tile" aspect="380 / 150" />
          ))}
        </div>
      ) : overview.isError && drives.length === 0 ? (
        <div role="alert" className="flex flex-1 flex-col items-start gap-2 px-6 pt-4">
          <p className="m-0 text-danger">{t("drivesPage.error")}</p>
          <Button onClick={() => void overview.refetch()}>{t("drivesPage.retry")}</Button>
        </div>
      ) : drives.length === 0 ? (
        <Empty />
      ) : (
        <>
          <div
            role="group"
            aria-label={t("drivesPage.cards")}
            className="flex w-[380px] shrink-0 flex-col gap-2.5 overflow-y-auto border-r border-solid border-line-section px-6 py-4"
          >
            <div className="flex items-center gap-2">
              <span className="type-section-label grow">{headerText(drives)}</span>
              <span className="text-(length:--fs-12) text-text-muted">{updated}</span>
              <Menu
                align="end"
                aria-label={t("drivesPage.menu")}
                trigger={
                  <IconButton size="sm" icon={MoreHorizontal} aria-label={t("drivesPage.menu")} />
                }
              >
                <MenuItem
                  icon={Trash2}
                  danger
                  {...(drives.some((entry) => entry.online && trashTotal(entry) > 0)
                    ? {}
                    : { disabledReason: t("drivesPage.trash.noneAnywhere") })}
                  onSelect={() => setEmptying("*")}
                >
                  {t("drivesPage.trash.emptyAll")}
                </MenuItem>
              </Menu>
            </div>
            {drives.map((entry) => (
              <DriveCard
                key={entry.id}
                drive={entry}
                selected={entry.id === drive?.id}
                now={now}
                onSelect={() => setChosen(entry.id)}
                onEmpty={() => setEmptying(entry.id)}
              />
            ))}
          </div>
          {drive === undefined ? null : <DriveTitles key={drive.id} drive={drive} />}
        </>
      )}
      {emptying === null ? null : (
        <EmptyTrashDialog
          driveId={emptying === "*" ? null : emptying}
          name={drives.find((entry) => entry.id === emptying)?.name ?? ""}
          onClose={() => setEmptying(null)}
        />
      )}
    </section>
  );
}
