/**
 * The pointer work of the Table's header: dragging a header to a new place, dragging an edge to
 * a new width, and fitting a column to its content. They change the DOM directly (transforms
 * and inline sizes on the header cell and the cells of the rows on screen, one animation frame
 * at a time) and tell React once, on release, so a drag in a table of thousands of rows never
 * renders a row. Every cell of a column carries `data-col="<id>"`.
 */
import { prefersReducedMotion } from "./motionHooks";
import { MAX_COLUMN_WIDTH, planDrop } from "./tableLayout";
import type { DropPlan, DropSlot } from "./tableLayout";

/** Pointer travel before a press on a header becomes a drag instead of a click. */
export const DRAG_THRESHOLD = 4;
const SLIDE = "transform var(--dur-page) var(--ease-out)";

/** The header cell and the visible body cells of one column. */
function cellsOf(grid: HTMLElement, id: string): HTMLElement[] {
  return Array.from(grid.querySelectorAll<HTMLElement>("[data-col]")).filter(
    (element) => element.dataset.col === id,
  );
}

function cellsByColumn(grid: HTMLElement): Map<string, HTMLElement[]> {
  const columns = new Map<string, HTMLElement[]>();
  for (const element of grid.querySelectorAll<HTMLElement>("[data-col]")) {
    const id = element.dataset.col ?? "";
    const list = columns.get(id);
    if (list === undefined) columns.set(id, [element]);
    else list.push(element);
  }
  return columns;
}

/** A pointer drag ends on Esc; the table's own Esc (clear the selection) must not see it. */
function onEscape(cancel: () => void): { stop: () => void } {
  const listener = (event: KeyboardEvent) => {
    if (event.key !== "Escape") return;
    event.preventDefault();
    event.stopPropagation();
    cancel();
  };
  window.addEventListener("keydown", listener, true);
  return { stop: () => window.removeEventListener("keydown", listener, true) };
}

interface HeaderDragOptions {
  grid: HTMLElement;
  /** The drop marker line in the header row, moved while dragging. */
  marker: HTMLElement | null;
  columnId: string;
  /** The columns in displayed order with the widths the table asked for. */
  columns: readonly { id: string; width: number; fixed: boolean }[];
  startX: number;
  /** The new displayed order, once the header is dropped somewhere new. */
  onCommit: (order: string[]) => void;
  /** Called once when the press ends; `dragged` is true when it was a drag, not a click. */
  onEnd: (dragged: boolean) => void;
}

/**
 * Follows a press on a header. Past a few pixels of travel the header lifts and follows the
 * pointer, the others slide to where they will sit, and a marker shows the place; releasing
 * commits the order, Esc puts everything back. Returns a function that cancels the gesture.
 */
export function beginHeaderDrag(options: HeaderDragOptions): () => void {
  const { grid, marker, columnId, columns, startX } = options;
  const cells = cellsByColumn(grid);
  // Widths as laid out (a growing column is wider than asked); a layout-less test sees 0 and
  // falls back to the width the table asked for.
  const widths = new Map(
    Array.from(grid.querySelectorAll<HTMLElement>("[data-column]")).map(
      (header) => [header.dataset.column ?? "", header.getBoundingClientRect().width] as const,
    ),
  );
  let left = 0;
  const slots: DropSlot[] = columns.map((column) => {
    const width = widths.get(column.id) || column.width;
    const slot = { id: column.id, left, width, fixed: column.fixed };
    left += width;
    return slot;
  });
  const own = slots.find((slot) => slot.id === columnId);
  if (own === undefined || own.fixed) return () => undefined;

  const reduced = prefersReducedMotion();
  let active = false;
  let lastX = startX;
  let frame = 0;
  let plan: DropPlan | null = null;
  let applied = "";

  const apply = () => {
    frame = 0;
    const next = planDrop(slots, columnId, own.left + lastX - startX);
    if (next === null) return;
    const key = next.order.join("|");
    for (const [id, list] of cells) {
      if (id !== columnId && key === applied) continue;
      const shift = id === columnId ? next.dragLeft - own.left : (next.shifts.get(id) ?? 0);
      const value = shift === 0 ? "" : `translateX(${shift}px)`;
      for (const element of list) element.style.transform = value;
    }
    applied = key;
    plan = next;
    if (marker !== null) marker.style.transform = `translateX(${next.dropLeft}px)`;
  };

  const lift = () => {
    active = true;
    grid.setAttribute("data-reordering", "");
    if (marker !== null) marker.style.display = "block";
    for (const [id, list] of cells) {
      for (const element of list) {
        if (id === columnId) element.setAttribute("data-lifted", "");
        else if (!reduced) element.style.transition = SLIDE;
      }
    }
  };

  const drop = () => {
    for (const list of cells.values()) {
      for (const element of list) {
        element.style.transform = "";
        element.style.transition = "";
        element.removeAttribute("data-lifted");
      }
    }
    grid.removeAttribute("data-reordering");
    if (marker !== null) {
      marker.style.display = "none";
      marker.style.transform = "";
    }
  };

  const escape = onEscape(() => finish(false));

  const move = (event: PointerEvent) => {
    lastX = event.clientX;
    if (!active) {
      if (Math.abs(lastX - startX) <= DRAG_THRESHOLD) return;
      lift();
    }
    if (frame === 0) frame = window.requestAnimationFrame(apply);
  };
  const up = (event: PointerEvent) => {
    lastX = event.clientX;
    finish(true);
  };
  const cancel = () => finish(false);

  function finish(commit: boolean) {
    window.removeEventListener("pointermove", move);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", cancel);
    escape.stop();
    if (frame !== 0) window.cancelAnimationFrame(frame);
    frame = 0;
    const dragged = active;
    if (dragged && commit) apply();
    drop();
    active = false;
    options.onEnd(dragged);
    const order = plan?.order;
    if (dragged && commit && order !== undefined) {
      if (order.join("|") !== columns.map((column) => column.id).join("|")) options.onCommit(order);
    }
  }

  window.addEventListener("pointermove", move);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", cancel);
  return cancel;
}

interface ResizeOptions {
  grid: HTMLElement;
  /** The divider being dragged; it holds the pointer. */
  handle: HTMLElement;
  pointerId: number;
  columnId: string;
  startX: number;
  /** The column's width on screen when the press began (a growing column may be wider than asked). */
  startWidth: number;
  minWidth: number;
  /** The row groups whose minimum width follows the table's width. */
  groups: readonly (HTMLElement | null)[];
  totalWidth: number;
  onCommit: (width: number) => void;
}

/**
 * Follows a press on a column's edge: the column's cells take the new width as the pointer moves
 * (one frame at a time, no React render), release commits it, Esc puts the old width back.
 */
export function beginResize(options: ResizeOptions): void {
  const { grid, handle, columnId, startX, startWidth, totalWidth } = options;
  const cells = cellsOf(grid, columnId);
  const groups = options.groups.filter((group): group is HTMLElement => group !== null);
  const saved = cells.map((element) => ({
    element,
    flex: element.style.flex,
    width: element.style.width,
    minWidth: element.style.minWidth,
  }));
  const savedGroups = groups.map((group) => group.style.minWidth);
  let latest = startWidth;
  let frame = 0;

  handle.setPointerCapture?.(options.pointerId);
  grid.setAttribute("data-resizing", "");

  const apply = () => {
    frame = 0;
    const size = `${latest}px`;
    for (const element of cells) {
      element.style.flex = `0 0 ${size}`;
      element.style.width = size;
      element.style.minWidth = size;
    }
    for (const group of groups) group.style.minWidth = `${totalWidth + latest - startWidth}px`;
  };
  const restore = () => {
    for (const entry of saved) {
      entry.element.style.flex = entry.flex;
      entry.element.style.width = entry.width;
      entry.element.style.minWidth = entry.minWidth;
    }
    groups.forEach((group, index) => (group.style.minWidth = savedGroups[index] ?? ""));
    grid.removeAttribute("data-resizing");
  };

  const escape = onEscape(() => finish(false));
  const move = (event: PointerEvent) => {
    latest = Math.max(
      options.minWidth,
      Math.min(MAX_COLUMN_WIDTH, Math.round(startWidth + event.clientX - startX)),
    );
    if (frame === 0) frame = window.requestAnimationFrame(apply);
  };
  const up = () => finish(true);
  const cancel = () => finish(false);

  function finish(commit: boolean) {
    handle.removeEventListener("pointermove", move);
    handle.removeEventListener("pointerup", up);
    handle.removeEventListener("pointercancel", cancel);
    handle.removeEventListener("lostpointercapture", up);
    escape.stop();
    if (frame !== 0) window.cancelAnimationFrame(frame);
    frame = 0;
    if (handle.hasPointerCapture?.(options.pointerId) === true) {
      handle.releasePointerCapture(options.pointerId);
    }
    restore();
    if (commit && Math.round(latest) !== Math.round(startWidth)) options.onCommit(latest);
  }

  handle.addEventListener("pointermove", move);
  handle.addEventListener("pointerup", up);
  handle.addEventListener("pointercancel", cancel);
  handle.addEventListener("lostpointercapture", up);
}

/**
 * The width a column needs for its widest visible cell and its header: each cell is laid out at
 * its natural size for a moment (no paint happens in between) and measured.
 */
export function measureColumn(grid: HTMLElement, columnId: string): number {
  const cells = cellsOf(grid, columnId);
  const saved = cells.map((element) => ({
    element,
    flex: element.style.flex,
    width: element.style.width,
    minWidth: element.style.minWidth,
    maxWidth: element.style.maxWidth,
  }));
  for (const element of cells) {
    element.style.flex = "none";
    element.style.width = "max-content";
    element.style.minWidth = "0";
    element.style.maxWidth = "none";
  }
  let widest = 0;
  for (const element of cells) widest = Math.max(widest, element.getBoundingClientRect().width);
  for (const entry of saved) {
    entry.element.style.flex = entry.flex;
    entry.element.style.width = entry.width;
    entry.element.style.minWidth = entry.minWidth;
    entry.element.style.maxWidth = entry.maxWidth;
  }
  return Math.ceil(widest);
}
