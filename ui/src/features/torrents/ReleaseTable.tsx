import { Copy, Info, Send } from "lucide-react";
import { useCallback, useEffect, useMemo } from "react";
import type { MouseEvent, ReactNode } from "react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { FileSize } from "../../components/FileSize";
import { MenuSeparator } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { QualityBadge } from "../../components/QualityBadge";
import { Table } from "../../components/Table";
import type { TableColumn, TableSort } from "../../components/Table";
import { Tag } from "../../components/Tag";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { sourceText, tierOf } from "./releaseSort";
import { copyMagnet, openRelease } from "./torrentActions";
import type { SourceInfo, TorrentRelease } from "./torrentBackend";
import { ageText, seedHealth } from "./torrentModel";

import "./torrentsPage.css";

/** What the library holds of the searched movie, for the hint on each release. */
export interface OwnedCopy {
  /** "2160p Dolby Vision". */
  quality: string;
  size: number | null;
}

interface ReleaseTableProps {
  releases: readonly TorrentRelease[];
  /** Every source of the search, to name the sources of a release. */
  sources: readonly Pick<SourceInfo, "id" | "name">[];
  owned: OwnedCopy | null;
  sort: TableSort;
  onSort: (sort: TableSort) => void;
  /**
   * Opens a release's details. Given, each row gets a Details button next to Send, and Enter and
   * double-click open the details instead of sending (a hand-off is always its own click).
   */
  onDetails?: (release: TorrentRelease) => void;
  /** Told the release when exactly one row is selected (a details panel follows it). */
  onSelectOne?: (release: TorrentRelease) => void;
  /** Lets the Quality column sort (best first). */
  qualitySortable?: boolean;
  /** More marks after the release name ("In your library"). */
  nameExtra?: (release: TorrentRelease) => ReactNode;
  /** Called with the rows on screen, for loading more near the end. */
  onVisibleRange?: (range: { start: number; end: number }) => void;
  "aria-label"?: string;
  /** Columns left out (a narrow table next to the details panel). */
  hiddenColumns?: ReadonlySet<string>;
  /** Keeps the column widths under this key; the search's own by default. */
  storageKey?: string;
}

/** The actions column fits its buttons at every window width: they never shrink or clip. */
export const SEND_COLUMN_WIDTH = 116;
export const ACTIONS_COLUMN_WIDTH = 148;

/** How a release compares with the copy the library holds, or null when there is none. */
function Compare({ release, owned }: { release: TorrentRelease; owned: OwnedCopy | null }) {
  const format = useFormat();
  const verdict = release.upgrade;
  if (verdict !== "same" && verdict !== "better" && verdict !== "worse") return null;
  const tip =
    owned === null
      ? undefined
      : t("torrentsPage.cmp.tip", {
          quality: owned.quality,
          size: owned.size === null ? "" : format.size(owned.size),
        });
  return (
    <span className="ml-2 shrink-0" {...(tip === undefined ? {} : { title: tip })}>
      <Tag variant={verdict === "better" ? "ok" : verdict === "worse" ? "warn" : "neutral"}>
        {t(`torrentsPage.cmp.${verdict}`)}
      </Tag>
    </span>
  );
}

/**
 * The release table (spec `torrent-search` "Release table columns and sort"): the
 * shared `Table` with Quality, Release name (the hint against the owned copy after it), Size,
 * Seeders, Source and Age, and an actions column sized to fit its buttons. Choosing a release
 * (double-click, Enter, the row's Send button or the right-click menu) hands its magnet link to the
 * torrent app; nothing is downloaded here. With `onDetails` (the "Latest releases" feed) Enter and
 * double-click open the details instead, and Send stays its own button.
 */
export function ReleaseTable({
  releases,
  sources,
  owned,
  sort,
  onSort,
  onDetails,
  onSelectOne,
  qualitySortable = false,
  nameExtra,
  onVisibleRange,
  hiddenColumns,
  "aria-label": ariaLabel = t("torrentsPage.table"),
  storageKey = "torrent-releases",
}: ReleaseTableProps) {
  const format = useFormat();
  const keys = useMemo(() => releases.map((release) => release.id), [releases]);
  const byId = useMemo(() => new Map(releases.map((release) => [release.id, release])), [releases]);
  const open = useCallback(
    (id: string) => {
      const release = byId.get(id);
      if (release === undefined) return;
      if (onDetails === undefined) void openRelease(release);
      else onDetails(release);
    },
    [byId, onDetails],
  );
  const listSelection = useListSelection({ keys, onOpen: open });
  // In the feed one plain click opens the details panel (again, too, after it was closed);
  // Ctrl/Shift-clicks only extend the selection, as in every other table.
  const selection = useMemo(() => {
    if (onDetails === undefined) return listSelection;
    return {
      ...listSelection,
      itemProps: (key: string) => {
        const props = listSelection.itemProps(key);
        return {
          ...props,
          onClick: (event: MouseEvent) => {
            props.onClick(event);
            if (event.ctrlKey || event.metaKey || event.shiftKey) return;
            const release = byId.get(key);
            if (release !== undefined) onDetails(release);
          },
        };
      },
    };
  }, [listSelection, onDetails, byId]);
  const [chosen] = [...selection.selected];
  const only = selection.count === 1 && chosen !== undefined ? byId.get(chosen) : undefined;
  useEffect(() => {
    if (only !== undefined) onSelectOne?.(only);
  }, [only, onSelectOne]);

  const columns = useMemo<TableColumn<TorrentRelease>[]>(
    () => [
      {
        id: "quality",
        label: t("torrentsPage.col.quality"),
        width: 104,
        sortable: qualitySortable,
        firstSort: "desc",
        cell: (release) => <QualityBadge quality={tierOf(release)} />,
      },
      {
        id: "name",
        label: t("torrentsPage.col.name"),
        width: 200,
        minWidth: 160,
        grow: true,
        cell: (release) => (
          <span className="flex min-w-0 items-center">
            <span className="cv-selectable min-w-0 truncate font-mono text-(length:--fs-13)">
              {release.name}
            </span>
            {nameExtra?.(release)}
            <Compare release={release} owned={owned} />
          </span>
        ),
      },
      {
        id: "size",
        label: t("torrentsPage.col.size"),
        width: 90,
        align: "end",
        sortable: true,
        cell: (release) => (release.size === null ? null : <FileSize bytes={release.size} />),
      },
      {
        id: "seeders",
        label: t("torrentsPage.col.seeders"),
        width: 100,
        align: "end",
        sortable: true,
        cell: (release) => (
          <span
            data-health={seedHealth(release.seeders)}
            className={cx(
              "font-(weight:--fw-600)",
              seedHealth(release.seeders) === 3
                ? "text-ok"
                : seedHealth(release.seeders) === 0
                  ? "text-text-muted"
                  : "text-text",
            )}
          >
            {release.seeders === null ? "" : format.number(release.seeders)}
          </span>
        ),
      },
      {
        id: "source",
        label: t("torrentsPage.col.source"),
        width: 120,
        sortable: true,
        cell: (release) => <span className="text-text-2">{sourceText(release, sources)}</span>,
      },
      {
        id: "age",
        label: t("torrentsPage.col.age"),
        width: 74,
        align: "end",
        sortable: true,
        cell: (release) => <span className="text-text-2">{ageText(release.published)}</span>,
      },
      {
        id: "actions",
        label: t("torrentsPage.actions"),
        header: <span className="sr-only">{t("torrentsPage.actions")}</span>,
        width: onDetails === undefined ? SEND_COLUMN_WIDTH : ACTIONS_COLUMN_WIDTH,
        resizable: false,
        fixed: true,
        bare: true,
        align: "end",
        cell: (release) => (
          <span data-testid="release-actions" className="flex shrink-0 items-center gap-1">
            {onDetails === undefined ? null : (
              <IconButton
                icon={Info}
                tabIndex={-1}
                aria-label={t("torrentsPage.details")}
                className="tr-send"
                onClick={(event) => {
                  event.stopPropagation();
                  onDetails(release);
                }}
                onDoubleClick={(event) => event.stopPropagation()}
              />
            )}
            <Button
              icon={Send}
              tabIndex={-1}
              aria-label={t("torrentsPage.send")}
              title={t(
                onDetails === undefined ? "torrentsPage.send.tip" : "torrentsPage.send.tipFeed",
              )}
              className="tr-send"
              onClick={(event) => {
                event.stopPropagation();
                void openRelease(release);
              }}
              onDoubleClick={(event) => event.stopPropagation()}
            >
              {t("torrentsPage.send.short")}
            </Button>
          </span>
        ),
      },
    ],
    [owned, sources, format, onDetails, qualitySortable, nameExtra],
  );
  const shownColumns = useMemo(
    () =>
      hiddenColumns === undefined
        ? columns
        : columns.filter((column) => !hiddenColumns.has(column.id)),
    [columns, hiddenColumns],
  );

  const menu = (
    <>
      {onDetails === undefined ? null : (
        <MenuItem
          icon={Info}
          shortcut="Enter"
          {...(only === undefined ? { disabledReason: t("torrentsPage.needOne") } : {})}
          onSelect={() => only !== undefined && onDetails(only)}
        >
          {t("torrentsPage.details")}
        </MenuItem>
      )}
      <MenuItem
        icon={Send}
        {...(onDetails === undefined ? { shortcut: "Enter" } : {})}
        {...(only === undefined ? { disabledReason: t("torrentsPage.needOne") } : {})}
        onSelect={() => only !== undefined && void openRelease(only)}
      >
        {t("torrentsPage.send")}
      </MenuItem>
      <MenuSeparator />
      <MenuItem
        icon={Copy}
        {...(only === undefined
          ? { disabledReason: t("torrentsPage.needOne") }
          : only.magnet === null
            ? { disabledReason: t("torrentsPage.noMagnet") }
            : {})}
        onSelect={() => only !== undefined && void copyMagnet(only)}
      >
        {t("torrentsPage.copy")}
      </MenuItem>
    </>
  );

  return (
    <Table
      aria-label={ariaLabel}
      rows={releases}
      rowKey={(release) => release.id}
      columns={shownColumns}
      selection={selection}
      storageKey={storageKey}
      {...(onVisibleRange === undefined ? {} : { onVisibleRange })}
      sort={sort}
      onSortChange={onSort}
      menu={menu}
      className="min-h-0 flex-1"
    />
  );
}
