import { TriangleAlert } from "lucide-react";
import type { KeyboardEvent } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { SpaceBar } from "../../components/SpaceBar";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { agoText, nameParts, qualityShares, trashTotal, usedShare } from "./drivesModel";
import type { Drive, DriveFolder } from "./drivesModel";

interface DriveCardProps {
  drive: Drive;
  selected: boolean;
  /** The current time, for "last seen 2 days ago". */
  now: number;
  onSelect: () => void;
  /** Opens the typed confirmation that empties this drive's trash. */
  onEmpty: () => void;
}

/** One muted line of the card: a name and a size at the right. */
function TrashLine({ label, bytes, mono }: { label: string; bytes: number; mono?: boolean }) {
  const format = useFormat();
  return (
    <div className="flex items-center gap-2 text-(length:--fs-13)">
      <span
        className={cx(
          "min-w-0 flex-1 truncate text-text-muted",
          mono === true && "cv-selectable font-mono text-(length:--fs-12)",
        )}
        title={label}
      >
        {label}
      </span>
      <span className="w-[62px] text-right text-text-muted tabular-nums">{format.size(bytes)}</span>
    </div>
  );
}

/** The segment colours of the quality bar, best first: four steps of grey, never a colour. */
const SEGMENT: Record<string, string> = {
  "2160p": "bg-text-2",
  "1080p": "bg-text-muted",
  "720p": "bg-text-disabled",
  SD: "bg-track",
};

function FolderRow({ folder, offline }: { folder: DriveFolder; offline: boolean }) {
  const format = useFormat();
  return (
    <div className="flex items-center gap-2 text-(length:--fs-13)">
      <span className="cv-selectable min-w-0 flex-1 truncate font-mono text-(length:--fs-12) text-text-2">
        {folder.path}
      </span>
      <span className={offline ? undefined : "text-text"}>
        {t(folder.type === "tv" ? "drivesPage.card.shows" : "drivesPage.card.titles", {
          count: folder.titles,
        })}
      </span>
      <span className="w-[62px] text-right text-text-muted tabular-nums">
        {format.size(folder.bytes)}
      </span>
    </div>
  );
}

/**
 * One drive of the Drives page (spec `drives` "Drive cards"): the name with its size
 * and kind, the free space (or the low warning), the used-space bar, each library folder with its
 * count and size, the space of other files, and the quality mix. Choosing the card selects the
 * drive (gold edge). An offline drive is greyed and says when it was last seen.
 */
export function DriveCard({ drive, selected, now, onSelect, onEmpty }: DriveCardProps) {
  const format = useFormat();
  const { head, rest } = nameParts(drive.name);
  const offline = !drive.online;
  const state = offline ? "offline" : drive.low ? "low" : "normal";
  const free = format.size(drive.freeBytes);
  const used = usedShare(drive);
  const shares = qualityShares(drive.quality);
  const spaceLabel = t("drivesPage.card.spaceLabel", {
    name: drive.name,
    percent: format.percent(used),
    free,
  });
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    if (event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      onSelect();
    }
  };
  return (
    <div
      role="button"
      tabIndex={0}
      aria-pressed={selected}
      aria-label={t("drivesPage.card.pick", { name: drive.name })}
      data-drive={drive.id}
      data-state={state}
      onClick={onSelect}
      onKeyDown={onKeyDown}
      className={cx(
        "cv-hoverable flex w-full cursor-pointer flex-col gap-[9px] overflow-hidden rounded-(--r-10)",
        "border border-solid border-line-section px-3.5 pt-3.5 pb-3 text-left",
        selected
          ? "bg-surface-raised shadow-[inset_2px_0_0_var(--gold)]"
          : "bg-surface-card hover:bg-surface-raised active:bg-surface-active",
        offline && "text-text-disabled",
      )}
    >
      <div className="flex items-center gap-2 whitespace-nowrap">
        {head === "" ? null : (
          <span className="text-(length:--fs-14) font-(weight:--fw-700)">{head}</span>
        )}
        <span className="shrink-0 font-(weight:--fw-600)">{rest}</span>
        {offline ? (
          <Tag>{t("drivesPage.card.offline")}</Tag>
        ) : (
          <span className="min-w-0 truncate text-(length:--fs-12) text-text-muted">
            ·{" "}
            {t("drivesPage.card.size", {
              size: format.size(drive.totalBytes),
              kind: t(`drivesPage.kind.${drive.kind}`),
            })}
          </span>
        )}
        <span className="grow" />
        {offline ? (
          <span className="text-(length:--fs-12) text-text-muted">
            {drive.lastSeenMs === null
              ? t("drivesPage.card.neverSeen")
              : t("drivesPage.card.lastSeen", {
                  when: agoText(drive.lastSeenMs, now, format.locale),
                })}
          </span>
        ) : drive.low ? (
          <span className="inline-flex items-center gap-[5px] text-(length:--fs-12) text-warn">
            <Icon icon={TriangleAlert} size={12} />
            {t("drivesPage.card.low", { free })}
          </span>
        ) : (
          <span className="text-(length:--fs-12) text-text-2">
            {t("drivesPage.card.free", { free, total: format.size(drive.totalBytes) })}
          </span>
        )}
      </div>
      <SpaceBar used={used} label={spaceLabel} state={state} size="md" />
      {drive.folders.map((folder) => (
        <FolderRow key={folder.sourceId} folder={folder} offline={offline} />
      ))}
      {drive.notInLibraryBytes === null ? null : (
        <div className="flex items-center gap-2 text-(length:--fs-13)">
          <span className="min-w-0 flex-1 text-text-muted">{t("drivesPage.card.other")}</span>
          <span className="w-[62px] text-right text-text-muted tabular-nums">
            {format.size(drive.notInLibraryBytes)}
          </span>
        </div>
      )}
      {offline ? null : (
        <>
          {drive.trashBytes > 0 ? (
            <TrashLine label={t("drivesPage.card.trash")} bytes={drive.trashBytes} />
          ) : null}
          {drive.binBytes > 0 ? (
            <TrashLine label={t("drivesPage.card.bin")} bytes={drive.binBytes} />
          ) : null}
          {drive.customTrash === null ? null : (
            <TrashLine
              label={t("drivesPage.card.customTrash", { path: drive.customTrash.path })}
              bytes={drive.customTrash.bytes}
            />
          )}
          {trashTotal(drive) > 0 ? (
            <div className="flex justify-end">
              <Button
                size="sm"
                variant="ghost"
                aria-label={t("drivesPage.card.emptyFor", { name: drive.name })}
                onClick={(event) => {
                  event.stopPropagation();
                  onEmpty();
                }}
              >
                {t("drivesPage.card.empty")}
              </Button>
            </div>
          ) : null}
        </>
      )}
      {shares.length === 0 || offline ? null : (
        <>
          <div
            role="img"
            aria-label={t("drivesPage.card.qualityLabel", {
              name: drive.name,
              breakdown: shares.map((share) => `${share.id} ${share.count}`).join(", "),
            })}
            className="flex h-1 gap-0.5 overflow-hidden rounded-(--r-pill)"
          >
            {shares.map((share) => (
              <div
                key={share.id}
                className={SEGMENT[share.id]}
                style={{ width: `${share.percent}%` }}
              />
            ))}
          </div>
          <div aria-hidden="true" className="flex gap-3 text-(length:--fs-12) text-text-muted">
            {shares.map((share) => (
              <span key={share.id}>{`${share.id} ${share.count}`}</span>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
