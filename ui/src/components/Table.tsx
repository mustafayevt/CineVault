import { useVirtualizer } from "@tanstack/react-virtual";
import { ArrowLeft, ArrowRight, ChevronDown, ChevronUp } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import { flushSync } from "react-dom";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { ContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
import { MenuSeparator } from "./Menu";
import { MenuItem } from "./MenuItem";
import { beginHeaderDrag, beginResize, measureColumn } from "./tableInteractions";
import {
  arrangeColumns,
  clampWidth,
  headerFloor,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  moveColumn,
  readLayout,
  RESIZE_STEP,
  writeLayout,
} from "./tableLayout";
import type { TableLayout } from "./tableLayout";
import type { ListKey, ListSelection } from "./useListSelection";

export interface TableColumn<T> {
  id: string;
  /** The header text, also the column's accessible name. */
  label: string;
  /** Header content drawn instead of the label (a select-all checkbox, an icon). */
  header?: ReactNode;
  /** The default width in CSS pixels; the user's width replaces it. */
  width: number;
  minWidth?: number;
  /** Takes the room left over in the table's width (the title column). */
  grow?: boolean;
  /** `end` for numbers: right-aligned with tabular figures. */
  align?: "start" | "end" | "center";
  sortable?: boolean;
  /** The direction a first click on this column sorts in; ascending by default. */
  firstSort?: "asc" | "desc";
  /** False for a column whose width never changes (a checkbox). Resizable by default. */
  resizable?: boolean;
  /** A trailing action column with no header text (Send): the header draws no divider before it. */
  bare?: boolean;
  /** A column that keeps its place when others are dragged (checkbox, stage). */
  fixed?: boolean;
  /** The cell's content. Text is cut with an ellipsis; mono literals add `cv-selectable` themselves. */
  cell: (row: T, index: number) => ReactNode;
}

export interface TableSort {
  column: string;
  direction: "asc" | "desc";
}

interface TableProps<T, K extends ListKey> {
  rows: readonly T[];
  rowKey: (row: T) => K;
  columns: readonly TableColumn<T>[];
  /** From `useListSelection` over the same keys, in the same order as `rows`. */
  selection: ListSelection<K>;
  "aria-label": string;
  /** Keeps the user's column order and widths across restarts under this key. */
  storageKey?: string;
  sort?: TableSort | null;
  /** A click on a sortable header: the same column flips direction, another sorts ascending. */
  onSortChange?: (sort: TableSort) => void;
  /** Row height; 34 by default (`--h-row`). */
  rowHeight?: number;
  /**
   * Room under the last row, so the floating selection bar never covers it: 16 by default; a page
   * showing the bar adds the bar's height.
   */
  bottomRoom?: number;
  /** The right-click menu of the selected rows (the same actions as the selection bar). */
  menu?: ReactNode;
  /** The right-click menu of the header row (a column chooser); the rows keep `menu`. */
  headerMenu?: ReactNode;
  /** Shown in place of the rows when there are none. */
  empty?: ReactNode;
  /** Called with the first and last row index on screen (with overscan) when they change. */
  onVisibleRange?: (range: { start: number; end: number }) => void;
  className?: string;
}

const HEADER_HEIGHT = 30;

/**
 * The one table: virtualised rows, a sticky 30px header in 12/600
 * muted text with sort arrows, columns the user can resize (drag the edge, double-click it to fit
 * the content, or arrows on its handle) and reorder (drag a header, "Move left/right" in the header
 * menu, or Alt+Left/Right), remembered under `storageKey`. Drags move the DOM directly and commit
 * once on release (see `tableInteractions`), so the rows never render while the pointer moves. Rows are 34px with
 * a hairline; hover and pressed take the next rungs, a selected row the gold tint, the current row a
 * 2px gold edge. Selection follows `useListSelection`; the menu key and right-click open `menu`.
 * The first and last columns sit on the 24px content inset.
 */
export function Table<T, K extends ListKey>({
  rows,
  rowKey,
  columns,
  selection,
  "aria-label": ariaLabel,
  storageKey,
  sort,
  onSortChange,
  rowHeight = 34,
  bottomRoom = 16,
  menu,
  headerMenu,
  empty,
  onVisibleRange,
  className,
}: TableProps<T, K>) {
  const id = useId();
  const scrollRef = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState<TableLayout>(() => readLayout(storageKey));
  const headerGroupRef = useRef<HTMLDivElement>(null);
  const bodyGroupRef = useRef<HTMLDivElement>(null);
  const markerRef = useRef<HTMLDivElement>(null);
  const cancelDrag = useRef<(() => void) | null>(null);
  const suppressClick = useRef(false);
  // A drag that is still going when the table goes away is ended, not left holding the window.
  useEffect(() => () => cancelDrag.current?.(), []);

  const saveLayout = (next: TableLayout) => {
    setLayout(next);
    writeLayout(storageKey, next);
  };

  const [menuOnHeader, setMenuOnHeader] = useState(false);
  const [menuColumn, setMenuColumn] = useState<string | null>(null);

  // A sortable column is never narrower than its label and sort arrow need.
  const floored = useMemo(
    () =>
      columns.map((column) => {
        const floor = headerFloor(column);
        if (floor === 0) return column;
        return {
          ...column,
          minWidth: Math.max(column.minWidth ?? 0, floor),
          width: Math.max(column.width, floor),
        };
      }),
    [columns],
  );
  // A column the user gave a width stops taking the room left over: its edge then follows the pointer.
  const arranged = useMemo(
    () =>
      arrangeColumns(floored, layout).map((column) =>
        column.grow === true && layout.widths[column.id] !== undefined
          ? { ...column, grow: false }
          : column,
      ),
    [floored, layout],
  );
  const fixedIds = useMemo(
    () => new Set(columns.filter((c) => c.fixed === true).map((c) => c.id)),
    [columns],
  );
  const shown = arranged;
  const totalWidth = shown.reduce((sum, c) => sum + c.width, 0);

  // TanStack Virtual returns functions the compiler cannot memoize; this component opts out of it.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: 8,
    scrollMargin: HEADER_HEIGHT,
    scrollPaddingStart: HEADER_HEIGHT,
    scrollPaddingEnd: bottomRoom,
  });
  const items = virtualizer.getVirtualItems();
  const firstShown = items[0]?.index ?? 0;
  const lastShown = items.at(-1)?.index ?? 0;
  useEffect(() => {
    if (rows.length > 0) onVisibleRange?.({ start: firstShown, end: lastShown });
  }, [firstShown, lastShown, rows.length, onVisibleRange]);

  // Keep the current row on screen when the keyboard moves it.
  const currentIndex = selection.currentIndex;
  useEffect(() => {
    if (currentIndex >= 0) virtualizer.scrollToIndex(currentIndex, { align: "auto" });
  }, [currentIndex, virtualizer]);

  const rowId = (index: number) => `${id}-row-${index}`;
  const currentShown = items.some((item) => item.index === currentIndex);

  const persistOrder = (order: string[]) => saveLayout({ ...layout, order });
  const moveBy = (columnId: string, delta: -1 | 1) => {
    const ids = arranged.map((c) => c.id);
    const order = moveColumn(ids, fixedIds, columnId, ids.indexOf(columnId) + delta);
    if (order !== null) persistOrder(order);
  };

  const resizeTo = (columnId: string, width: number, min?: number) =>
    saveLayout({ ...layout, widths: { ...layout.widths, [columnId]: clampWidth(width, min) } });

  const startHeaderDrag = (columnId: string, event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || fixedIds.has(columnId)) return;
    if ((event.target as HTMLElement).closest('[role="separator"]') !== null) return;
    const grid = scrollRef.current;
    if (grid === null) return;
    // Window listeners, not pointer capture: a press that does not move stays a click on the
    // header's sort button.
    cancelDrag.current?.();
    cancelDrag.current = beginHeaderDrag({
      grid,
      marker: markerRef.current,
      columnId,
      columns: arranged.map((c) => ({ id: c.id, width: c.width, fixed: fixedIds.has(c.id) })),
      startX: event.clientX,
      onEnd: (dragged) => {
        cancelDrag.current = null;
        if (!dragged) return;
        // The click that follows a drag's release is not a sort.
        suppressClick.current = true;
        window.setTimeout(() => (suppressClick.current = false), 0);
      },
      onCommit: (order) => flushSync(() => persistOrder(order)),
    });
  };

  const startResize = (
    column: (typeof arranged)[number],
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    const grid = scrollRef.current;
    if (event.button !== 0 || grid === null) return;
    event.preventDefault();
    event.stopPropagation();
    const header = event.currentTarget.closest<HTMLElement>("[data-column]");
    beginResize({
      grid,
      handle: event.currentTarget,
      pointerId: event.pointerId,
      columnId: column.id,
      startX: event.clientX,
      startWidth: header?.getBoundingClientRect().width || column.width,
      minWidth: column.minWidth ?? MIN_COLUMN_WIDTH,
      groups: [headerGroupRef.current, bodyGroupRef.current],
      totalWidth,
      onCommit: (width) => flushSync(() => resizeTo(column.id, width, column.minWidth)),
    });
  };

  const fitColumn = (column: (typeof arranged)[number]) => {
    const grid = scrollRef.current;
    if (grid === null) return;
    const width = measureColumn(grid, column.id);
    if (width > 0) resizeTo(column.id, width, column.minWidth);
  };

  const onSort = (column: TableColumn<T>) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    const first = column.firstSort ?? "asc";
    const flipped = first === "asc" ? ("desc" as const) : ("asc" as const);
    const direction =
      sort?.column === column.id ? (sort.direction === first ? flipped : first) : first;
    onSortChange?.({ column: column.id, direction });
  };

  const onGridKeyDown = (event: ReactKeyboardEvent<HTMLDivElement>) => {
    // Keys on a header's controls (sort, resize, move) are theirs.
    if (event.target !== event.currentTarget) return;
    if (selection.onKeyDown(event)) {
      event.preventDefault();
      if (event.key === "Escape") event.stopPropagation();
    }
  };

  const keyOfElement = (element: Element): K | null => {
    const index = element.closest<HTMLElement>("[data-index]")?.dataset.index;
    const row = index === undefined ? undefined : rows[Number(index)];
    return row === undefined ? null : rowKey(row);
  };

  const cellAlign = (align: TableColumn<T>["align"]) =>
    align === "end"
      ? "justify-end text-right tabular-nums"
      : align === "center"
        ? "justify-center text-center"
        : "justify-start";
  const edgePadding = (index: number) =>
    cx(index === 0 ? "pl-6" : "pl-3", index === shown.length - 1 ? "pr-6" : "pr-3");
  const columnStyle = (column: { width: number; grow?: boolean }) =>
    column.grow === true
      ? { flex: `1 0 ${column.width}px`, minWidth: column.width }
      : { flex: `0 0 ${column.width}px`, width: column.width };

  const grid = (
    // The grid keeps focus itself and points at the current row (aria-activedescendant), so the
    // virtualised rows can come and go without losing it.
    <div
      ref={scrollRef}
      role="grid"
      aria-label={ariaLabel}
      aria-rowcount={rows.length + 1}
      aria-colcount={shown.length}
      aria-multiselectable="true"
      aria-activedescendant={currentShown ? rowId(currentIndex) : undefined}
      tabIndex={0}
      onKeyDown={onGridKeyDown}
      className={cx(
        "relative min-h-0 overflow-auto bg-surface-window text-(length:--fs-13) text-text outline-none",
        // While a header is dragged the whole table shows the grabbing hand.
        "[&[data-reordering]_*]:cursor-grabbing! [&[data-resizing]_*]:cursor-col-resize!",
        className,
      )}
    >
      <div
        ref={headerGroupRef}
        role="rowgroup"
        className="sticky top-0 z-10"
        style={{ minWidth: totalWidth }}
      >
        <div
          role="row"
          aria-rowindex={1}
          data-table-header=""
          className="relative flex h-(--h-thead) border-b border-solid border-line-section bg-surface-window select-none"
        >
          {/* Where the dragged header would land; moved by the drag, hidden otherwise. */}
          <div
            ref={markerRef}
            aria-hidden="true"
            data-drop-marker=""
            style={{ display: "none" }}
            className="pointer-events-none absolute top-0 left-0 z-30 h-full w-0.5 bg-gold"
          />
          {shown.map((column, index) => {
            const sorted = sort?.column === column.id ? sort.direction : null;
            const last = index === shown.length - 1;
            return (
              // The header cell is not a control itself: its sort button and resize handle are.
              // eslint-disable-next-line jsx-a11y/interactive-supports-focus
              <div
                key={column.id}
                role="columnheader"
                aria-colindex={index + 1}
                aria-label={column.label}
                aria-sort={
                  column.sortable !== true
                    ? undefined
                    : sorted === null
                      ? "none"
                      : sorted === "asc"
                        ? "ascending"
                        : "descending"
                }
                data-column={column.id}
                data-col={column.id}
                onPointerDown={(event) => startHeaderDrag(column.id, event)}
                onKeyDown={(event) => {
                  if (!event.altKey || column.fixed === true) return;
                  if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                  event.preventDefault();
                  moveBy(column.id, event.key === "ArrowLeft" ? -1 : 1);
                }}
                style={columnStyle(column)}
                className={cx(
                  "relative flex h-full min-w-0 items-center text-(length:--fs-12) font-(weight:--fw-600) whitespace-nowrap text-text-muted",
                  !last &&
                    shown[index + 1]?.bare !== true &&
                    "border-r border-solid border-line-row",
                  "data-[lifted]:z-20 data-[lifted]:bg-surface-card data-[lifted]:shadow-floating",
                )}
              >
                {column.sortable === true ? (
                  <button
                    type="button"
                    aria-label={column.header === undefined ? undefined : column.label}
                    onClick={() => onSort(column)}
                    className={cx(
                      "cv-hoverable flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1",
                      "hover:bg-surface-card hover:text-text focus-visible:text-text",
                      edgePadding(index),
                      cellAlign(column.align),
                      column.align === "end" && "flex-row-reverse",
                      sorted !== null && "text-text",
                    )}
                  >
                    <span className="min-w-0 truncate">{column.header ?? column.label}</span>
                    {/* The arrow has a place of its own, so the label never gives way to it. */}
                    <span
                      aria-hidden="true"
                      data-sort-slot=""
                      className="inline-flex size-3 shrink-0"
                    >
                      {sorted !== null && (
                        <Icon icon={sorted === "asc" ? ChevronUp : ChevronDown} size={12} />
                      )}
                    </span>
                  </button>
                ) : (
                  <span
                    className={cx(
                      "flex h-full min-w-0 flex-1 items-center",
                      edgePadding(index),
                      cellAlign(column.align),
                    )}
                  >
                    {column.header ?? <span className="truncate">{column.label}</span>}
                  </span>
                )}
                {column.resizable !== false && (
                  // A focusable separator is the ARIA window-splitter pattern: arrows resize.
                  // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions
                  <div
                    role="separator"
                    aria-orientation="vertical"
                    aria-label={t("library.table.resize", { column: column.label })}
                    aria-valuemin={column.minWidth ?? MIN_COLUMN_WIDTH}
                    aria-valuemax={MAX_COLUMN_WIDTH}
                    aria-valuenow={column.width}
                    // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex
                    tabIndex={0}
                    onPointerDown={(event) => startResize(column, event)}
                    // Double-clicking the edge fits the column to its widest visible content.
                    onDoubleClick={() => fitColumn(column)}
                    onKeyDown={(event) => {
                      const direction =
                        event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
                      if (direction === 0 || event.altKey) return;
                      event.preventDefault();
                      event.stopPropagation();
                      resizeTo(column.id, column.width + direction * RESIZE_STEP, column.minWidth);
                    }}
                    className={cx(
                      "absolute top-0 z-10 h-full w-2.5 cursor-col-resize touch-none outline-none",
                      "focus-visible:bg-gold-tint-edge hover:bg-line-edge",
                      last ? "right-0" : "-right-[5px]",
                    )}
                  />
                )}
              </div>
            );
          })}
        </div>
      </div>
      {rows.length === 0 && empty !== undefined ? (
        <div className="px-6 py-4 text-text-muted">{empty}</div>
      ) : (
        <div
          ref={bodyGroupRef}
          role="rowgroup"
          className="relative"
          style={{ height: virtualizer.getTotalSize() + bottomRoom, minWidth: totalWidth }}
        >
          {items.map((item) => {
            const row = rows[item.index];
            if (row === undefined) return null;
            const key = rowKey(row);
            const selected = selection.isSelected(key);
            const current = item.index === currentIndex;
            const pointer = selection.itemProps(key);
            return (
              // Rows are reached with the grid's arrow keys (it holds focus); clicks select.
              // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
              <div
                key={String(key)}
                id={rowId(item.index)}
                role="row"
                aria-rowindex={item.index + 2}
                aria-selected={selected}
                data-index={item.index}
                data-current={current ? "true" : undefined}
                onClick={pointer.onClick}
                onDoubleClick={pointer.onDoubleClick}
                onMouseDown={(event) => {
                  // Shift-click selects rows, never text; the grid still takes focus.
                  if (event.shiftKey) {
                    event.preventDefault();
                    scrollRef.current?.focus();
                  }
                }}
                className={cx(
                  "cv-hoverable absolute top-0 left-0 flex w-full cursor-pointer border-b border-solid border-line-row",
                  selected ? "bg-selected-row" : "hover:bg-surface-card",
                  "active:bg-surface-raised",
                  current && "shadow-[inset_2px_0_0_var(--gold)]",
                )}
                style={{
                  height: item.size,
                  transform: `translateY(${item.start - HEADER_HEIGHT}px)`,
                }}
              >
                {shown.map((column, index) => (
                  <div
                    key={column.id}
                    role="gridcell"
                    aria-colindex={index + 1}
                    data-col={column.id}
                    style={columnStyle(column)}
                    className={cx(
                      "relative flex h-full min-w-0 items-center overflow-hidden whitespace-nowrap",
                      "data-[lifted]:z-10 data-[lifted]:bg-surface-card",
                      edgePadding(index),
                      cellAlign(column.align),
                    )}
                  >
                    <span className="min-w-0 truncate">{column.cell(row, item.index)}</span>
                  </div>
                ))}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );

  // "Move left" and "Move right" for the column that was right-clicked: the keyboard path to a
  // reorder, next to the column chooser. Fixed columns and the ends of the row have nothing to move to.
  const menuIds = arranged.map((c) => c.id);
  const menuIndex = menuColumn === null ? -1 : menuIds.indexOf(menuColumn);
  const canMove = (delta: -1 | 1) =>
    menuColumn !== null && moveColumn(menuIds, fixedIds, menuColumn, menuIndex + delta) !== null;
  const moveItems =
    menuColumn === null || fixedIds.has(menuColumn) ? null : (
      <>
        <MenuItem
          icon={ArrowLeft}
          disabledReason={canMove(-1) ? undefined : t("library.table.firstPlace")}
          onSelect={() => moveBy(menuColumn, -1)}
        >
          {t("library.table.moveLeft")}
        </MenuItem>
        <MenuItem
          icon={ArrowRight}
          disabledReason={canMove(1) ? undefined : t("library.table.lastPlace")}
          onSelect={() => moveBy(menuColumn, 1)}
        >
          {t("library.table.moveRight")}
        </MenuItem>
        <MenuSeparator />
      </>
    );

  if (menu === undefined && headerMenu === undefined) return grid;
  return (
    <ContextMenu
      menu={
        menuOnHeader && headerMenu !== undefined ? (
          <>
            {moveItems}
            {headerMenu}
          </>
        ) : (
          menu
        )
      }
      onTarget={(target) => {
        const onHeader = target.closest("[data-table-header]") !== null;
        setMenuOnHeader(onHeader);
        setMenuColumn(
          onHeader ? (target.closest<HTMLElement>("[data-column]")?.dataset.column ?? null) : null,
        );
        if (onHeader) return;
        const key = keyOfElement(target);
        if (key !== null) selection.target(key);
      }}
      keyboardTarget={() =>
        scrollRef.current?.querySelector('[role="row"][data-current="true"]') ?? null
      }
    >
      {grid}
    </ContextMenu>
  );
}
