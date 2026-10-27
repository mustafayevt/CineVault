/**
 * The user's arrangement of a Table: column order and widths, kept under a storage key so it
 * survives a restart (spec desktop-interaction "Column width kept"). Stored in the webview's own
 * storage; a damaged or missing entry falls back to the table's defaults.
 */

export interface TableLayout {
  /** Column ids in the user's order; ids the table no longer has are ignored. */
  order: string[];
  /** Widths the user gave, in CSS pixels. */
  widths: Record<string, number>;
}

export const MIN_COLUMN_WIDTH = 40;
export const MAX_COLUMN_WIDTH = 960;
/** Arrow keys on a column's resize handle change its width by this much. */
export const RESIZE_STEP = 16;

const PREFIX = "cinevault.table.";

const emptyLayout = (): TableLayout => ({ order: [], widths: {} });

export function clampWidth(width: number, min = MIN_COLUMN_WIDTH): number {
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(min, width)));
}

/** The stored layout for `key`, or an empty one. */
export function readLayout(key: string | undefined): TableLayout {
  if (key === undefined) return emptyLayout();
  try {
    const raw = window.localStorage.getItem(PREFIX + key);
    if (raw === null) return emptyLayout();
    const value: unknown = JSON.parse(raw);
    if (typeof value !== "object" || value === null) return emptyLayout();
    const { order, widths } = value as Partial<Record<keyof TableLayout, unknown>>;
    const layout = emptyLayout();
    if (Array.isArray(order)) layout.order = order.filter((id) => typeof id === "string");
    if (typeof widths === "object" && widths !== null) {
      for (const [id, width] of Object.entries(widths)) {
        if (typeof width === "number" && Number.isFinite(width)) layout.widths[id] = width;
      }
    }
    return layout;
  } catch {
    return emptyLayout();
  }
}

/** Saves the layout for `key`; storage that is full or switched off only loses the preference. */
export function writeLayout(key: string | undefined, layout: TableLayout): void {
  if (key === undefined) return;
  try {
    window.localStorage.setItem(PREFIX + key, JSON.stringify(layout));
  } catch {
    // Nothing to do: the table keeps working with the layout in memory.
  }
}

/** Pixels a 12px semibold header letter takes, rounded up so a wide capital still fits. */
const HEADER_LETTER = 7.2;
/** The header button's padding (12 and up to 24 at the table's ends), the 4px gap and the arrow. */
const HEADER_CHROME = 12 + 24 + 4 + 12;

/**
 * The narrowest a sortable column can be before its label gives way to the sort arrow: 0 for a
 * column that is not sortable or draws its own header content.
 */
export function headerFloor(column: {
  label: string;
  sortable?: boolean;
  header?: unknown;
}): number {
  if (column.sortable !== true || column.header !== undefined) return 0;
  return Math.ceil(column.label.length * HEADER_LETTER + HEADER_CHROME);
}

interface ColumnLike {
  id: string;
  width: number;
  minWidth?: number;
  /** A column that stays where it is (a checkbox or status column). */
  fixed?: boolean;
}

/**
 * The columns in the user's order with the user's widths. Fixed columns keep their own place;
 * the movable ones fill the other places in the stored order, columns the stored order does not
 * know yet after them.
 */
export function arrangeColumns<C extends ColumnLike>(
  columns: readonly C[],
  layout: TableLayout,
): (C & { width: number })[] {
  const movableColumns = columns.filter((column) => column.fixed !== true);
  const byId = new Map(movableColumns.map((column) => [column.id, column]));
  const stored = [...new Set(layout.order)].flatMap((id) => {
    const column = byId.get(id);
    return column === undefined ? [] : [column];
  });
  const storedIds = new Set(stored.map((column) => column.id));
  const movable = [...stored, ...movableColumns.filter((column) => !storedIds.has(column.id))];
  let next = 0;
  return columns.map((column) => {
    const placed = column.fixed === true ? column : (movable[next++] ?? column);
    const width = layout.widths[placed.id];
    return {
      ...placed,
      width: width === undefined ? placed.width : clampWidth(width, placed.minWidth),
    };
  });
}

/**
 * The order after moving column `id` to `toIndex` among `ids` (the displayed order). Fixed
 * columns neither move nor give up their place; null when nothing changes.
 */
export function moveColumn(
  ids: readonly string[],
  fixed: ReadonlySet<string>,
  id: string,
  toIndex: number,
): string[] | null {
  const from = ids.indexOf(id);
  if (from === -1 || fixed.has(id)) return null;
  const target = Math.max(0, Math.min(ids.length - 1, toIndex));
  if (target === from || fixed.has(ids[target] ?? "")) return null;
  const next = ids.filter((other) => other !== id);
  next.splice(target, 0, id);
  return next;
}
