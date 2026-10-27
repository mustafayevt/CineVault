import { useMemo } from "react";
import type { KeyboardEvent } from "react";

import { Checkbox } from "../../components/Checkbox";
import { FileSize } from "../../components/FileSize";
import { QualityBadge } from "../../components/QualityBadge";
import { Skeleton } from "../../components/Skeleton";
import { StageBadge } from "../../components/StageBadge";
import { Table } from "../../components/Table";
import type { TableColumn, TableSort } from "../../components/Table";
import { Tooltip } from "../../components/Tooltip";
import { CachedImage } from "../images/CachedImage";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { usePreferOriginalTitle } from "./displayTitle";
import type { LibraryRow, LibrarySortKey } from "./libraryBackend";
import { LibraryColumnItems } from "./LibraryColumnItems";
import {
  COLUMN_LABEL,
  COLUMN_SORT,
  columnOfField,
  DESCENDING_FIRST,
  SWITCHABLE_COLUMNS,
} from "./libraryColumns";
import type { LibraryColumnId } from "./libraryColumns";
import {
  cardTitle,
  libraryFolderOf,
  problemNote,
  qualityTier,
  ratingText,
  runtimeText,
  stageBadgeOf,
  yearText,
} from "./titleFacts";
import type { ListViewProps } from "./viewTypes";

/** One line of the table: the title's id and its row once its page has arrived. */
interface Slot {
  id: string;
  row: LibraryRow | undefined;
}

interface LibraryTableProps extends ListViewProps {
  sort: readonly LibrarySortKey[];
  onSort: (sort: readonly LibrarySortKey[]) => void;
  hiddenColumns: readonly string[];
  /** A column switched on or off from the header's right-click menu. */
  onColumn: (id: string, shown: boolean) => void;
}

function Loading({ width }: { width: string }) {
  return <Skeleton shape="line" width={width} />;
}

/**
 * The Library table: the shared `Table` with a select-all checkbox, Title with
 * its thumbnail and warning, Year, Quality, Size, Drive and folder, Rating and Runtime, and the
 * optional Stage, Date added and File modified columns. Every column sorts the whole library in
 * the backend. Widths and order are remembered per type, the sort is the one of the grid, the
 * selection, the menu and the keys are the grid's, and a right-click on the header opens the
 * column chooser.
 */
export function LibraryTable({
  type,
  listing,
  selection,
  menu,
  bottomRoom,
  empty,
  onKey,
  sort,
  onSort,
  hiddenColumns,
  onColumn,
}: LibraryTableProps) {
  const preferOriginal = usePreferOriginalTitle();
  const format = useFormat();
  const { keys, version, total } = listing;

  // Rebuilt when the ids or a page arrive, not on every render.
  const slots = useMemo<Slot[]>(
    () => keys.map((id, index) => ({ id, row: listing.rowAt(index) })),
    // `rowAt` reads the pages whose arrival is `version`.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [keys, version],
  );

  const columns = useMemo<TableColumn<Slot>[]>(() => {
    const label = (id: LibraryColumnId) => t(COLUMN_LABEL[id]);
    const first = (id: LibraryColumnId) =>
      DESCENDING_FIRST.has(id) ? { firstSort: "desc" as const } : {};
    const dateCell = (ms: number | null | undefined) => {
      if (ms === undefined || ms === null || ms <= 0) return null;
      // The short date in the cell, the full date and time on hover.
      return (
        <Tooltip content={format.dateTime(ms)}>
          <span className="text-text-2">{format.date(ms)}</span>
        </Tooltip>
      );
    };
    const defined: TableColumn<Slot>[] = [
      {
        id: "select",
        label: t("library.column.select"),
        header: (
          <Checkbox
            aria-label={t("library.column.selectAll")}
            checked={selection.count > 0 && selection.count === keys.length}
            indeterminate={selection.count > 0 && selection.count < keys.length}
            onCheckedChange={(checked) => (checked ? selection.selectAll() : selection.clear())}
          />
        ),
        width: 50,
        fixed: true,
        resizable: false,
        cell: (slot) => (
          // The checkbox acts as a Ctrl+click: it adds or removes this title only.
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
          <span
            className="flex items-center"
            onClick={(event) => event.stopPropagation()}
            onDoubleClick={(e) => e.stopPropagation()}
          >
            <Checkbox
              aria-label={t("library.row.select", {
                title: slot.row === undefined ? "" : cardTitle(slot.row.card, preferOriginal),
              })}
              checked={selection.isSelected(slot.id)}
              onCheckedChange={() => selection.click(slot.id, { toggle: true })}
            />
          </span>
        ),
      },
      {
        id: "stage",
        label: label("stage"),
        header: <span aria-hidden="true" />,
        width: 34,
        fixed: true,
        resizable: false,
        sortable: true,
        cell: (slot) =>
          slot.row === undefined ? null : (
            <StageBadge
              stage={stageBadgeOf(slot.row.card)}
              label={problemNote(slot.row.card) ?? undefined}
            />
          ),
      },
      {
        id: "title",
        label: label("title"),
        width: 280,
        minWidth: 160,
        grow: true,
        sortable: true,
        ...first("title"),
        cell: (slot) => {
          const row = slot.row;
          if (row === undefined) return <Loading width="60%" />;
          const name = cardTitle(row.card, preferOriginal);
          const note = problemNote(row.card);
          return (
            <span className="flex min-w-0 items-center gap-2.5">
              <span className="h-[30px] w-5 shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active">
                {row.card.poster !== null && (
                  <CachedImage
                    imageKey={row.card.poster.key}
                    size="list"
                    alt=""
                    thumbhash={row.card.poster.thumbhash}
                    ambient={row.card.poster.ambient}
                    className="size-full"
                  />
                )}
              </span>
              <span className="truncate font-(weight:--fw-500)">{name}</span>
              {note !== null && (
                <span className="shrink-0 text-(length:--fs-12) text-warn">{note}</span>
              )}
            </span>
          );
        },
      },
      {
        id: "year",
        label: label("year"),
        width: 64,
        align: "end",
        sortable: true,
        ...first("year"),
        cell: (slot) =>
          slot.row === undefined ? null : (
            <span className="text-text-2">{yearText(slot.row.card.year).replace("????", "")}</span>
          ),
      },
      {
        id: "quality",
        label: label("quality"),
        width: 150,
        sortable: true,
        ...first("quality"),
        cell: (slot) => {
          const card = slot.row?.card;
          if (card === undefined) return null;
          const tier = qualityTier(card.resolution);
          if (tier === null) return null;
          return (
            <span className="flex items-center gap-1.5">
              <QualityBadge quality={tier} />
              {card.hdr !== null && (
                <span className="text-(length:--fs-12) text-text-muted">{card.hdr}</span>
              )}
            </span>
          );
        },
      },
      {
        id: "size",
        label: label("size"),
        width: 90,
        align: "end",
        sortable: true,
        ...first("size"),
        cell: (slot) => {
          const value = slot.row?.total_file_size ?? slot.row?.file_size;
          return value === undefined || value === null || value <= 0 ? null : (
            <FileSize bytes={value} />
          );
        },
      },
      {
        id: "folder",
        label: label("folder"),
        width: 200,
        minWidth: 100,
        sortable: true,
        ...first("folder"),
        cell: (slot) =>
          slot.row === undefined ? null : (
            <span className="cv-selectable font-mono text-(length:--fs-12) text-text-2">
              {libraryFolderOf(slot.row.path)}
            </span>
          ),
      },
      {
        id: "rating",
        label: label("rating"),
        width: 70,
        align: "end",
        sortable: true,
        ...first("rating"),
        cell: (slot) => (slot.row === undefined ? null : ratingText(slot.row)),
      },
      {
        id: "runtime",
        label: label("runtime"),
        width: 84,
        align: "end",
        sortable: true,
        ...first("runtime"),
        cell: (slot) =>
          slot.row === undefined ? null : (
            <span className="text-text-2">{runtimeText(slot.row.runtime_minutes)}</span>
          ),
      },
      {
        id: "added",
        label: label("added"),
        width: 120,
        align: "end",
        sortable: true,
        ...first("added"),
        cell: (slot) => dateCell(slot.row?.date_added_ms),
      },
      {
        id: "modified",
        label: label("modified"),
        width: 130,
        align: "end",
        sortable: true,
        ...first("modified"),
        cell: (slot) => dateCell(slot.row?.file_modified_ms),
      },
    ];
    return defined.filter((column) => !hiddenColumns.includes(column.id));
  }, [selection, keys.length, hiddenColumns, preferOriginal, format]);

  const chooser = useMemo(
    () =>
      SWITCHABLE_COLUMNS.map((id) => ({
        id,
        label: t(COLUMN_LABEL[id]),
        shown: !hiddenColumns.includes(id),
      })),
    [hiddenColumns],
  );

  const first = sort[0];
  const sorted: TableSort | null = useMemo(() => {
    const column = first === undefined ? null : columnOfField(first.field);
    return first === undefined || column === null
      ? null
      : { column, direction: first.descending ? "desc" : "asc" };
  }, [first]);

  const onSortChange = (next: TableSort) => {
    const field = COLUMN_SORT[next.column as LibraryColumnId];
    onSort([{ field, descending: next.direction === "desc" }]);
  };

  // F2 and Delete are the list's own keys; the grid and the table share them. The shared table
  // handles the selection keys itself.
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if ((event.target as HTMLElement).getAttribute("role") !== "grid") return;
    if (onKey(event)) event.preventDefault();
  };

  return (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div className="flex min-h-0 flex-1 flex-col" onKeyDown={onKeyDown}>
      <Table
        aria-label={t("library.table.label", {
          kind: t(type === "movies" ? "library.type.movies" : "library.type.tv"),
        })}
        rows={slots}
        rowKey={(slot) => slot.id}
        columns={columns}
        selection={selection}
        storageKey={`library-${type}`}
        sort={sorted}
        onSortChange={onSortChange}
        bottomRoom={bottomRoom}
        menu={menu}
        headerMenu={<LibraryColumnItems columns={chooser} onColumn={onColumn} />}
        {...(total === 0 && !listing.loading ? { empty } : {})}
        onVisibleRange={listing.setRange}
        className="min-h-0 flex-1"
      />
    </div>
  );
}
