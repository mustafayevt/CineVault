import { ChevronDown, LayoutGrid, RefreshCw, Table2 } from "lucide-react";

import { Button } from "../../components/Button";
import { IconButton } from "../../components/IconButton";
import { Menu, MenuRadioGroup, MenuRadioItem } from "../../components/Menu";
import { SegmentedControl } from "../../components/SegmentedControl";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { ReloadMediaInfoButton } from "../mediainfo/ReloadMediaInfoButton";
import type { LibrarySortKey } from "./libraryBackend";
import { SORT_CHOICES, SORT_KEYS, SORT_LABEL, sortChoiceOf } from "./sortChoices";
import type { SortChoice } from "./sortChoices";
import type { LibraryType, LibraryView } from "./libraryStore";
import type { PosterSize } from "./gridGeometry";
import { LibraryColumnItems } from "./LibraryColumnItems";
import type { ColumnChoice } from "./LibraryColumnItems";
import type { PaneFit } from "./usePaneFit";

interface LibraryToolbarProps {
  type: LibraryType;
  /** The three counts of the type switch; undefined while unknown. */
  counts: { movies?: number; tv?: number; collections?: number };
  onType: (type: LibraryType) => void;
  /** How much room the pane has; on a phone the table view sorts from the sort menu too. */
  fit?: PaneFit;
  view: LibraryView;
  onView: (view: LibraryView) => void;
  /** How wide the grid's posters are; the control shows only in the grid view. */
  posterSize: PosterSize;
  onPosterSize: (size: PosterSize) => void;
  sort: readonly LibrarySortKey[];
  onSort: (sort: readonly LibrarySortKey[]) => void;
  /** The table's columns for the Columns menu. */
  columns: readonly ColumnChoice[];
  onColumn: (id: string, shown: boolean) => void;
  scanning: boolean;
  onScan: () => void;
}

/**
 * The toolbar: the Movies | TV shows | Collections
 * switch with counts, then at the right the sort menu (grid) or the Columns menu (table), the
 * grid and table switch, Reload media info and the scan button. 48px high with the section hairline under it, on the
 * 24px content inset, controls on one centre line. In a phone-wide pane it takes two rows: the type
 * switch across the width, then the sort menu at the left and the view switch and buttons at the
 * right; the poster size gives way there (two posters fill the row at any size but L).
 */
export function LibraryToolbar({
  type,
  counts,
  onType,
  fit = "wide",
  view,
  onView,
  posterSize,
  onPosterSize,
  sort,
  onSort,
  columns,
  onColumn,
  scanning,
  onScan,
}: LibraryToolbarProps) {
  const format = useFormat();
  const count = (value: number | undefined) =>
    value === undefined ? undefined : format.number(value);
  const choice = sortChoiceOf(sort);
  const titles = type !== "collections";
  // On a phone the table keeps only the title, quality and stage: the sort menu sorts by the rest.
  const sortMenu = titles && (view === "grid" || fit === "phone");
  return (
    <div
      className={cx(
        "relative flex h-12 shrink-0 items-center gap-2.5 px-6",
        "@max-[640px]/pane:h-auto @max-[640px]/pane:flex-wrap @max-[640px]/pane:gap-y-2 @max-[640px]/pane:py-2",
      )}
    >
      {/* The section hairline starts and ends at the content inset. */}
      <span aria-hidden="true" className="absolute right-6 bottom-0 left-6 h-px bg-line-section" />
      <SegmentedControl<LibraryType>
        aria-label={t("library.type.label")}
        value={type}
        onChange={onType}
        className="@max-[640px]/pane:flex @max-[640px]/pane:w-full @max-[640px]/pane:[&>button]:flex-1"
        options={[
          { value: "movies", label: t("library.type.movies"), count: count(counts.movies) },
          { value: "tv", label: t("library.type.tv"), count: count(counts.tv) },
          {
            value: "collections",
            label: t("library.type.collections"),
            count: count(counts.collections),
          },
        ]}
      />
      <span className="grow @max-[640px]/pane:hidden" />

      {sortMenu && (
        <Menu
          align="end"
          aria-label={t("library.sortBy.label")}
          trigger={
            <Button variant="ghost" trailingIcon={ChevronDown}>
              {choice === null ? t("library.sortBy.custom") : t(SORT_LABEL[choice])}
            </Button>
          }
        >
          <MenuRadioGroup
            value={choice ?? ""}
            onValueChange={(value) => onSort(SORT_KEYS[value as SortChoice])}
          >
            {SORT_CHOICES.map((entry) => (
              <MenuRadioItem key={entry} value={entry}>
                {t(SORT_LABEL[entry])}
              </MenuRadioItem>
            ))}
          </MenuRadioGroup>
        </Menu>
      )}
      {titles && view === "table" && !sortMenu && (
        <Menu
          align="end"
          aria-label={t("library.columns.label")}
          trigger={
            <Button variant="ghost" trailingIcon={ChevronDown}>
              {t("library.columns.button")}
            </Button>
          }
        >
          <LibraryColumnItems columns={columns} onColumn={onColumn} />
        </Menu>
      )}
      {!titles && (
        <span className="px-3 text-(length:--fs-13) font-(weight:--fw-500) text-text-2">
          {t("library.sortBy.collections")}
        </span>
      )}

      {titles && (
        <>
          <span
            aria-hidden="true"
            className="h-[18px] w-px bg-line-section @max-[640px]/pane:ml-auto"
          />
          {view === "grid" && (
            <SegmentedControl<PosterSize>
              aria-label={t("library.posterSize.label")}
              className="@max-[640px]/pane:hidden"
              value={posterSize}
              onChange={onPosterSize}
              options={[
                { value: "s", label: "S", ariaLabel: t("library.posterSize.small") },
                { value: "m", label: "M", ariaLabel: t("library.posterSize.medium") },
                { value: "l", label: "L", ariaLabel: t("library.posterSize.large") },
              ]}
            />
          )}
          <SegmentedControl<LibraryView>
            aria-label={t("library.view.label")}
            value={view}
            onChange={onView}
            options={[
              { value: "grid", icon: LayoutGrid, ariaLabel: t("library.view.grid") },
              { value: "table", icon: Table2, ariaLabel: t("library.view.table") },
            ]}
          />
        </>
      )}
      {titles && <ReloadMediaInfoButton />}
      <IconButton
        icon={RefreshCw}
        aria-label={t("library.scan")}
        disabled={scanning}
        onClick={onScan}
        {...(titles ? {} : { className: "@max-[640px]/pane:ml-auto" })}
      />
    </div>
  );
}
