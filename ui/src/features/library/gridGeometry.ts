/**
 * Geometry of the poster grid: posters at least 148px wide (so a 1440 window with its scrollbar still shows seven), 18px apart, rows 22px
 * apart, on the 24px content inset, so 1440 wide shows seven across. Rows have one fixed height,
 * so the scroll position of any row is arithmetic and nothing is measured while scrolling.
 */
const MIN_POSTER_WIDTH = 148;
const COLUMN_GAP = 18;
const ROW_GAP = 22;
/** The inset on each side of the grid. */
const INSET = 24;
/** Poster aspect ratio: height over width. */
const POSTER_RATIO = 1.5;
/** Under the poster: the 7px gap, the 13px title line, the 12px meta line less its 6px pull-up. */
const CAPTION_HEIGHT = 44;

export interface GridGeometry {
  columns: number;
  /** Width of one poster in px. */
  cellWidth: number;
  /** Height of one row in px, including the gap to the next row. */
  rowHeight: number;
}

/** The columns that fit `containerWidth`, the poster width that fills the row, the row height. */
export function gridGeometry(containerWidth: number): GridGeometry {
  const usable = Math.max(0, containerWidth - INSET * 2);
  const columns = Math.max(1, Math.floor((usable + COLUMN_GAP) / (MIN_POSTER_WIDTH + COLUMN_GAP)));
  const cellWidth = Math.max(1, (usable - COLUMN_GAP * (columns - 1)) / columns);
  const rowHeight = Math.round(cellWidth * POSTER_RATIO + CAPTION_HEIGHT + ROW_GAP);
  return { columns, cellWidth, rowHeight };
}
