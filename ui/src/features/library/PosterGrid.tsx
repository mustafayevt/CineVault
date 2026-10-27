import { useVirtualizer } from "@tanstack/react-virtual";
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import { ContextMenu } from "../../components/ContextMenu";
import { PosterTile } from "../../components/PosterTile";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { CachedImage } from "../images/CachedImage";
import { usePreferOriginalTitle } from "./displayTitle";
import { gridGeometry, POSTER_SIZES } from "./gridGeometry";
import { useLibraryStore } from "./libraryStore";
import { cardTitle, problemNote, qualityTier, stageBadgeOf, yearText } from "./titleFacts";
import type { LibraryRow } from "./libraryBackend";
import type { ListViewProps } from "./viewTypes";
import "./library.css";

/** Rows drawn beyond the viewport on each side, so a fast fling meets drawn rows, not blanks. */
const ROW_OVERSCAN = 4;

interface PosterGridProps extends ListViewProps {
  /** Tells the page how many posters fit in a row, for the arrow keys. */
  onColumns: (columns: number) => void;
}

/** The artwork over its ThumbHash preview, or undefined so the tile draws the generated poster. */
function posterNode(row: LibraryRow): ReactNode | undefined {
  const poster = row.card.poster;
  if (poster === null) return undefined;
  return (
    <CachedImage
      imageKey={poster.key}
      size="grid"
      alt=""
      thumbhash={poster.thumbhash}
      ambient={poster.ambient}
      className="size-full"
    />
  );
}

/**
 * The poster grid: virtualised rows of `PosterTile`s on the 24px
 * inset, seven across at 1440 wide. Click selects, Ctrl and Shift extend, double-click and Enter
 * open; the arrow keys move the current poster (a keyboard-only ring), and the right-click menu
 * is the one of the selection bar. Rows have one fixed height, so scrolling is arithmetic.
 */
export function PosterGrid({
  type,
  listing,
  selection,
  menu,
  bottomRoom,
  empty,
  onKey,
  onColumns,
}: PosterGridProps) {
  const scrollRef = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const preferOriginal = usePreferOriginalTitle();

  useLayoutEffect(() => {
    const element = scrollRef.current;
    if (element === null) return;
    const measure = () => setWidth(element.clientWidth);
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const posterSize = useLibraryStore((store) => store.posterSize);
  const { columns, cellWidth, rowHeight } = gridGeometry(width, POSTER_SIZES[posterSize]);
  const total = listing.total;
  const rowCount = Math.ceil(total / columns);

  // TanStack Virtual returns functions the compiler cannot memoize; this component opts out of it.
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => rowHeight,
    overscan: ROW_OVERSCAN,
    paddingEnd: bottomRoom,
  });
  const items = virtualizer.getVirtualItems();
  const firstRow = items[0]?.index ?? 0;
  const lastRow = items.at(-1)?.index ?? 0;

  const { setRange } = listing;
  useEffect(() => {
    if (total > 0) setRange({ start: firstRow * columns, end: (lastRow + 1) * columns - 1 });
  }, [firstRow, lastRow, columns, total, setRange]);
  useEffect(() => onColumns(columns), [columns, onColumns]);

  // The row height changes with the width: tell the virtualizer to measure again.
  useEffect(() => {
    virtualizer.measure();
  }, [rowHeight, virtualizer]);

  // A new poster size keeps the same title at the top: without this the list would jump to wherever
  // the old scroll position falls among the new, taller or shorter rows.
  const topIndex = useRef(0);
  const lastSize = useRef(posterSize);
  useEffect(() => {
    if (lastSize.current !== posterSize) {
      lastSize.current = posterSize;
      virtualizer.scrollToIndex(Math.floor(topIndex.current / columns), { align: "start" });
    }
  }, [posterSize, columns, virtualizer]);
  // Declared after the effect above, so that one still sees the top title of the previous layout.
  useEffect(() => {
    topIndex.current = firstRow * columns;
  });

  const currentIndex = selection.currentIndex;
  useEffect(() => {
    if (currentIndex >= 0) virtualizer.scrollToIndex(Math.floor(currentIndex / columns));
  }, [currentIndex, columns, virtualizer]);

  const keyOf = (index: number) => listing.keys[index];
  const tileId = (index: number) => `poster-${index}`;

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    if (event.target !== event.currentTarget) return;
    if (selection.onKeyDown(event) || onKey(event)) {
      event.preventDefault();
      if (event.key === "Escape") event.stopPropagation();
    }
  };

  /* eslint-disable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */
  const grid = (
    // The grid holds focus itself and points at the current poster, so tiles can come and go.
    <div
      ref={scrollRef}
      role="group"
      aria-label={t("library.grid.aria", {
        type: t(type === "movies" ? "library.type.movies" : "library.type.tv"),
      })}
      aria-busy={listing.loading}
      data-testid="poster-scroll"
      data-rows={rowCount}
      data-columns={columns}
      aria-activedescendant={currentIndex >= 0 ? tileId(currentIndex) : undefined}
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="cv-poster-grid min-h-0 flex-1 overflow-x-hidden overflow-y-auto px-6 pt-1.5 outline-none select-none"
    >
      {total === 0 && !listing.loading ? (
        empty
      ) : (
        <div className="relative" style={{ height: virtualizer.getTotalSize() }}>
          {items.map((item) => (
            <div
              key={item.key}
              className="absolute top-0 left-0 grid w-full gap-x-[18px]"
              style={{
                height: item.size,
                gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
                transform: `translateY(${item.start}px)`,
                alignContent: "start",
              }}
            >
              {Array.from({ length: columns }, (_, column) => {
                const index = item.index * columns + column;
                if (index >= total) return null;
                const row = listing.rowAt(index);
                const key = keyOf(index) ?? `#${index}`;
                if (row === undefined) {
                  return (
                    <div key={key} data-index={index} style={{ width: cellWidth }}>
                      <Skeleton shape="poster" />
                    </div>
                  );
                }
                const card = row.card;
                const name = cardTitle(card, preferOriginal);
                const quality = qualityTier(card.resolution);
                const note = problemNote(card);
                const pointer = selection.itemProps(key);
                const art = posterNode(row);
                const current = index === currentIndex;
                return (
                  <div
                    key={key}
                    id={tileId(index)}
                    data-index={index}
                    data-id={key}
                    data-current={current ? "true" : undefined}
                    className="cv-poster-cell min-w-0"
                  >
                    <PosterTile
                      title={name}
                      year={yearText(card.year)}
                      {...(quality === null ? {} : { quality })}
                      {...(note === null ? {} : { note })}
                      stage={stageBadgeOf(card)}
                      {...(note === null ? {} : { stageLabel: note })}
                      selected={selection.isSelected(key)}
                      {...(art === undefined ? {} : { poster: art })}
                      tabIndex={-1}
                      onClick={(event) => {
                        scrollRef.current?.focus();
                        pointer.onClick(event);
                      }}
                      onDoubleClick={pointer.onDoubleClick}
                      draggable={false}
                    />
                  </div>
                );
              })}
            </div>
          ))}
        </div>
      )}
    </div>
  );

  /* eslint-enable jsx-a11y/no-noninteractive-element-interactions, jsx-a11y/no-noninteractive-tabindex */

  return (
    <ContextMenu
      menu={menu}
      onTarget={(target) => {
        const id = target.closest<HTMLElement>("[data-id]")?.dataset.id;
        if (id !== undefined) selection.target(id);
      }}
      keyboardTarget={() => scrollRef.current?.querySelector('[data-current="true"]') ?? null}
    >
      {grid}
    </ContextMenu>
  );
}
