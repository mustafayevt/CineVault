import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronDown, ChevronUp } from "lucide-react";
import { useEffect, useId, useMemo, useRef, useState } from "react";
import type {
  KeyboardEvent as ReactKeyboardEvent,
  PointerEvent as ReactPointerEvent,
  ReactNode,
} from "react";

import { t } from "../i18n/catalog";
import { cx } from "../lib/cx";
import { ContextMenu } from "./ContextMenu";
import { Icon } from "./Icon";
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
const DRAG_THRESHOLD = 4;

interface Drag {
  id: string;
  startX: number;
  moved: boolean;
  /** Where the header would land in the displayed order. */
  to: number;
}

/**
 * The one table: virtualised rows, a sticky 30px header in 12/600
 * muted text with sort arrows, columns the user can resize (drag the edge, or arrows on its handle)
 * and reorder (drag a header, or Alt+Left/Right), remembered under `storageKey`. Rows are 34px with
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
  const headerRef = useRef<HTMLDivElement>(null);
  const [layout, setLayout] = useState<TableLayout>(() => readLayout(storageKey));
  const [live, setLive] = useState<{ id: string; width: number } | null>(null);
  const [drag, setDrag] = useState<Drag | null>(null);
  const suppressClick = useRef(false);

  const saveLayout = (next: TableLayout) => {
    setLayout(next);
    writeLayout(storageKey, next);
  };

  const [menuOnHeader, setMenuOnHeader] = useState(false);

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
  const arranged = useMemo(() => arrangeColumns(floored, layout), [floored, layout]);
  const fixedIds = useMemo(
    () => new Set(columns.filter((c) => c.fixed === true).map((c) => c.id)),
    [columns],
  );
  const shown = useMemo(() => {
    const sized = arranged.map((c) => (live?.id === c.id ? { ...c, width: live.width } : c));
    if (drag === null || !drag.moved) return sized;
    const order = moveColumn(
      sized.map((c) => c.id),
      fixedIds,
      drag.id,
      drag.to,
    );
    if (order === null) return sized;
    return order.flatMap((columnId) => sized.filter((c) => c.id === columnId));
  }, [arranged, live, drag, fixedIds]);
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

  /**
   * Where a dragged header would be inserted, from the pointer's x over the columns' resting
   * places (not the live order, which would move under the pointer while it drags).
   */
  const dropIndex = (clientX: number): number => {
    const left = headerRef.current?.getBoundingClientRect().left ?? 0;
    let edge = left;
    let index = 0;
    for (const column of arranged) {
      if (clientX >= edge + column.width / 2) index += 1;
      edge += column.width;
    }
    return index;
  };

  const startHeaderDrag = (columnId: string, event: ReactPointerEvent<HTMLDivElement>) => {
    if (event.button !== 0 || fixedIds.has(columnId)) return;
    if ((event.target as HTMLElement).closest('[role="separator"]') !== null) return;
    // Window listeners, not pointer capture: a press that does not move stays a click on the
    // header's sort button.
    const from = arranged.findIndex((c) => c.id === columnId);
    let state: Drag = { id: columnId, startX: event.clientX, moved: false, to: from };
    const move = (moveEvent: PointerEvent) => {
      const moved = state.moved || Math.abs(moveEvent.clientX - state.startX) > DRAG_THRESHOLD;
      if (!moved) return;
      const over = dropIndex(moveEvent.clientX);
      state = { ...state, moved, to: over > from ? over - 1 : over };
      setDrag(state);
    };
    const end = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      setDrag(null);
      if (!state.moved) return;
      suppressClick.current = true;
      const order = moveColumn(
        arranged.map((c) => c.id),
        fixedIds,
        state.id,
        state.to,
      );
      if (order !== null) persistOrder(order);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  };

  const startResize = (
    column: (typeof arranged)[number],
    event: ReactPointerEvent<HTMLDivElement>,
  ) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.stopPropagation();
    const handle = event.currentTarget;
    handle.setPointerCapture?.(event.pointerId);
    const startX = event.clientX;
    let latest = column.width;
    const move = (moveEvent: PointerEvent) => {
      latest = clampWidth(column.width + moveEvent.clientX - startX, column.minWidth);
      setLive({ id: column.id, width: latest });
    };
    const end = () => {
      handle.removeEventListener("pointermove", move);
      handle.removeEventListener("pointerup", end);
      handle.removeEventListener("pointercancel", end);
      setLive(null);
      if (latest !== column.width) resizeTo(column.id, latest, column.minWidth);
    };
    handle.addEventListener("pointermove", move);
    handle.addEventListener("pointerup", end);
    handle.addEventListener("pointercancel", end);
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
        className,
      )}
    >
      <div role="rowgroup" className="sticky top-0 z-10" style={{ minWidth: totalWidth }}>
        <div
          ref={headerRef}
          role="row"
          aria-rowindex={1}
          data-table-header=""
          className="flex h-(--h-thead) border-b border-solid border-line-section bg-surface-window"
        >
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
                data-dragging={drag?.moved === true && drag.id === column.id ? "" : undefined}
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
                  "data-[dragging]:bg-surface-card",
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
                    onDoubleClick={() => {
                      // Double-clicking the edge gives the column its default width back.
                      const widths = Object.fromEntries(
                        Object.entries(layout.widths).filter(
                          ([columnId]) => columnId !== column.id,
                        ),
                      );
                      saveLayout({ ...layout, widths });
                    }}
                    onKeyDown={(event) => {
                      const direction =
                        event.key === "ArrowRight" ? 1 : event.key === "ArrowLeft" ? -1 : 0;
                      if (direction === 0 || event.altKey) return;
                      event.preventDefault();
                      event.stopPropagation();
                      resizeTo(column.id, column.width + direction * RESIZE_STEP, column.minWidth);
                    }}
                    className={cx(
                      "absolute top-0 z-10 h-full w-2 cursor-col-resize outline-none",
                      "focus-visible:bg-gold-tint-edge hover:bg-line-edge",
                      last ? "right-0" : "-right-1",
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
                    style={columnStyle(column)}
                    className={cx(
                      "flex h-full min-w-0 items-center overflow-hidden whitespace-nowrap",
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

  if (menu === undefined && headerMenu === undefined) return grid;
  return (
    <ContextMenu
      menu={menuOnHeader && headerMenu !== undefined ? headerMenu : menu}
      onTarget={(target) => {
        const onHeader = target.closest("[data-table-header]") !== null;
        setMenuOnHeader(onHeader);
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
