import { Copy, Info, Scale, Send } from "lucide-react";
import { useCallback, useEffect, useMemo, useState } from "react";
import type { MouseEvent, ReactNode } from "react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { FileSize } from "../../components/FileSize";
import { MenuSeparator } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { Popover } from "../../components/Popover";
import { QualityBadge } from "../../components/QualityBadge";
import { Table } from "../../components/Table";
import type { TableColumn, TableSort } from "../../components/Table";
import { Tag } from "../../components/Tag";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { usePaneWidth } from "../../lib/usePaneWidth";
import { CompareBlock } from "./CompareBlock";
import { reasonOf, verdictOf } from "./ownedCopy";
import type { LibraryCopy } from "./ownedCopy";
import { sourceText, tierOf } from "./releaseSort";
import { copyMagnet, openRelease } from "./torrentActions";
import type { SourceInfo, TorrentRelease } from "./torrentBackend";
import { ageText, seedHealth } from "./torrentModel";

import "./torrentsPage.css";

interface ReleaseTableProps {
  releases: readonly TorrentRelease[];
  /** Every source of the search, to name the sources of a release. */
  sources: readonly Pick<SourceInfo, "id" | "name">[];
  /** The library copy the releases are compared with; without one no release has a tag. */
  owned: LibraryCopy | null;
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

/** Below this pane width (a tablet) the source and age are left out; the release says them. */
const NARROW_PANE = 860;
/** Below this one (a phone) the quality moves into the name and the seeders go too. */
const PHONE_PANE = 640;
const NARROW_HIDDEN: readonly string[] = ["source", "age"];
const PHONE_HIDDEN: readonly string[] = ["quality", "seeders", "source", "age"];
/** The actions column of a phone: the Send icon alone. */
const PHONE_ACTIONS_WIDTH = 52;
/** A phone row holds the name over its marks. */
const PHONE_ROW_HEIGHT = 52;

interface CompareProps {
  release: TorrentRelease;
  owned: LibraryCopy | null;
  open: boolean;
  onOpenChange: (open: boolean) => void;
}

/**
 * How a release compares with the copy the library holds: a tag whose tooltip says why, and
 * which opens "Compare with yours". Nothing without a compared copy.
 */
function Compare({ release, owned, open, onOpenChange }: CompareProps) {
  const verdict = verdictOf(release);
  if (owned === null || verdict === null) return null;
  const label = t(`torrentsPage.cmp.${verdict}`);
  const why = reasonOf(verdict, owned, release);
  return (
    <span className="ml-2 shrink-0">
      <Popover
        aria-label={t("torrentsPage.cmp.title")}
        open={open}
        onOpenChange={onOpenChange}
        className="w-[360px] max-w-[calc(100vw-32px)]"
        trigger={
          <button
            type="button"
            // The row is the focus stop; the right-click menu's "Compare with yours" opens it too.
            tabIndex={-1}
            title={why}
            aria-label={`${label}. ${t("torrentsPage.cmp.open")}`}
            data-testid="compare-tag"
            className="flex cursor-pointer rounded-(--r-4) border-0 bg-transparent p-0"
            onClick={(event) => event.stopPropagation()}
            onDoubleClick={(event) => event.stopPropagation()}
          >
            <Tag variant={verdict === "better" ? "ok" : verdict === "worse" ? "warn" : "neutral"}>
              {label}
            </Tag>
          </button>
        }
      >
        {/* Keys inside the popover stay there (React events cross the portal): Enter never
            sends the row behind it. The popover's own Esc still closes it. */}
        {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions */}
        <div onKeyDown={(event) => event.stopPropagation()}>
          <CompareBlock copy={owned} release={release} verdict={verdict} heading />
        </div>
      </Popover>
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
  // The release whose "Compare with yours" is open.
  const [comparing, setComparing] = useState<string | null>(null);
  const { ref: frame, width: pane } = usePaneWidth();
  const narrow = pane !== null && pane <= NARROW_PANE;
  const phone = pane !== null && pane < PHONE_PANE;
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
        cell: (release) =>
          phone ? (
            // A phone has no room for a line of marks after the name: they go under it, after
            // the quality.
            <span className="flex min-w-0 flex-col gap-1">
              <span className="cv-selectable min-w-0 truncate font-mono text-(length:--fs-13)">
                {release.name}
              </span>
              <span className="flex min-w-0 items-center overflow-hidden">
                <QualityBadge quality={tierOf(release)} />
                {nameExtra?.(release)}
                <Compare
                  release={release}
                  owned={owned}
                  open={comparing === release.id}
                  onOpenChange={(open) => setComparing(open ? release.id : null)}
                />
              </span>
            </span>
          ) : (
            <span className="flex min-w-0 items-center">
              <span className="cv-selectable min-w-0 truncate font-mono text-(length:--fs-13)">
                {release.name}
              </span>
              {nameExtra?.(release)}
              <Compare
                release={release}
                owned={owned}
                open={comparing === release.id}
                onOpenChange={(open) => setComparing(open ? release.id : null)}
              />
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
        width: phone
          ? PHONE_ACTIONS_WIDTH + (onDetails === undefined ? 0 : 36)
          : onDetails === undefined
            ? SEND_COLUMN_WIDTH
            : ACTIONS_COLUMN_WIDTH,
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
              {phone ? null : t("torrentsPage.send.short")}
            </Button>
          </span>
        ),
      },
    ],
    [owned, sources, format, onDetails, qualitySortable, nameExtra, phone, comparing],
  );
  const shownColumns = useMemo(() => {
    const kept = columns.filter(
      (column) =>
        hiddenColumns?.has(column.id) !== true &&
        !(phone ? PHONE_HIDDEN : narrow ? NARROW_HIDDEN : []).includes(column.id),
    );
    if (!narrow || pane === null) return kept;
    // A narrow pane starts the name at the room the others leave (it still grows), so the table
    // never scrolls sideways.
    const others = kept.reduce(
      (sum, column) => (column.id === "name" ? sum : sum + column.width),
      0,
    );
    return kept.map((column) =>
      column.id === "name"
        ? {
            ...column,
            minWidth: phone ? 120 : (column.minWidth ?? 0),
            width: Math.min(column.width, Math.max(phone ? 120 : 160, pane - others - 24)),
          }
        : column,
    );
  }, [columns, hiddenColumns, narrow, phone, pane]);

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
      {owned === null ? null : (
        <MenuItem
          icon={Scale}
          {...(only === undefined
            ? { disabledReason: t("torrentsPage.needOne") }
            : verdictOf(only) === null
              ? { disabledReason: t("torrentsPage.cmp.none") }
              : {})}
          onSelect={() => {
            if (only === undefined) return;
            // After the menu has closed and given focus back, or the popover would close at once.
            // The menu gives focus back on a timer of its own, scheduled after this one, so the
            // popover waits one more turn.
            const id = only.id;
            window.setTimeout(() => window.setTimeout(() => setComparing(id), 0), 0);
          }}
        >
          {t("torrentsPage.cmp.open")}
        </MenuItem>
      )}
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
    // The frame only tells the pane width; it takes no room of its own.
    <div ref={frame} className="contents">
      <Table
        aria-label={ariaLabel}
        rows={releases}
        rowKey={(release) => release.id}
        columns={shownColumns}
        selection={selection}
        storageKey={storageKey}
        {...(phone ? { rowHeight: PHONE_ROW_HEIGHT } : {})}
        {...(onVisibleRange === undefined ? {} : { onVisibleRange })}
        sort={sort}
        onSortChange={onSort}
        menu={menu}
        className="min-h-0 flex-1"
      />
    </div>
  );
}
