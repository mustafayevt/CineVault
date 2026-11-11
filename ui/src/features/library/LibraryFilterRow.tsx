import { useQuery } from "@tanstack/react-query";
import { ChevronDown, Copy, TriangleAlert, Wrench, X } from "lucide-react";

import { Chip } from "../../components/Chip";
import {
  Menu,
  MenuCheckboxItem,
  MenuRadioGroup,
  MenuRadioItem,
  MenuSeparator,
} from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { useLifecycleCounts } from "../lifecycle/useLifecycleCounts";
import { currentLibraryBackend } from "./libraryBackend";
import type { LibraryKind, LibraryQuery } from "./libraryBackend";
import { FIX_FACETS } from "./libraryStore";
import type {
  FixChoice,
  LibraryFilters,
  ProblemChoice,
  QualityChoice,
  StageChoice,
} from "./libraryStore";

const STAGES: readonly StageChoice[] = ["all", "new", "scraped", "renamed"];
const QUALITIES: readonly QualityChoice[] = ["any", "2160p", "1080p", "720p", "sd"];
const PROBLEMS: readonly Exclude<ProblemChoice, "any">[] = [
  "all",
  "not-matched",
  "uncertain",
  "missing",
];

const STAGE_LABEL = {
  all: "library.stageChips.all",
  new: "stage.new",
  scraped: "stage.scraped",
  renamed: "stage.renamed",
} as const;

const QUALITY_LABEL = {
  any: "library.quality.any",
  "2160p": "library.quality.2160p",
  "1080p": "library.quality.1080p",
  "720p": "library.quality.720p",
  sd: "library.quality.sd",
} as const;

const PROBLEM_LABEL = {
  all: "library.problems.all",
  "not-matched": "library.problems.notMatched",
  uncertain: "library.problems.uncertain",
  missing: "library.problems.missing",
} as const;

/** The Needs fixing entries in menu order; "not-scraped" is the New stage, the rest are catalogue chips. */
const FIX_ENTRIES: readonly (FixChoice | "not-scraped")[] = [
  "not-scraped",
  "poster",
  "fanart",
  "clearlogo",
  "plot",
  "title",
];

const FIX_LABEL = {
  "not-scraped": "library.fix.notScraped",
  poster: "library.fix.poster",
  fanart: "library.fix.fanart",
  clearlogo: "library.fix.clearlogo",
  plot: "library.fix.plot",
  title: "library.fix.title",
} as const;

/**
 * How many titles of `query` miss each thing of the Needs fixing menu, by the catalogue's own
 * counts (`missing_artwork` and `missing_metadata`). The query leaves the menu's own choices out,
 * so every entry counts what picking it alone would list. Empty where the backend has no
 * catalogue counts.
 */
function useFixCounts(query: LibraryQuery): ReadonlyMap<FixChoice, number> {
  const counts = useQuery({
    // Under "library", so everything that changes artwork or metadata refreshes it.
    queryKey: ["library", "needs-fixing", query] as const,
    queryFn: async () => {
      const backend = currentLibraryBackend();
      const read = backend.facetValues;
      const result = new Map<FixChoice, number>();
      if (read === undefined) return result;
      const ids = [...new Set(Object.values(FIX_FACETS).map((facet) => facet.id))];
      const lists = await Promise.all(ids.map(async (id) => [id, await read(query, id)] as const));
      for (const [choice, { id, value }] of Object.entries(FIX_FACETS) as [
        FixChoice,
        (typeof FIX_FACETS)[FixChoice],
      ][]) {
        const values = lists.find(([listed]) => listed === id)?.[1] ?? [];
        const found = values.find((entry) => entry.value.toLowerCase() === value.toLowerCase());
        result.set(choice, found?.count ?? 0);
      }
      return result;
    },
    staleTime: 30_000,
  });
  return counts.data ?? new Map<FixChoice, number>();
}

interface LibraryFilterRowProps {
  kind: Exclude<LibraryKind, "collections">;
  filters: LibraryFilters;
  /** The query the row sits over without its Needs fixing choices, for the menu's counts. */
  countsQuery: LibraryQuery;
  onChange: (change: Partial<LibraryFilters>) => void;
  /** The typed search of the title bar (`?q=`); shown as a chip that clears it. */
  text: string;
  onClearText: () => void;
  /** "181 movies · 7.06 TB". */
  footer: string;
}

/**
 * The filter row: the stage chips All, New, Scraped and Renamed with
 * their counts, then the Drive, Quality, Problems and Needs fixing dropdown chips, the typed search as a chip,
 * and at the right the count and size of what is listed. 42px high on the 24px content inset, chips
 * 24px. On a tablet or a phone the chips scroll sideways inside their own row, out to the pane's
 * edges, and on a phone the count and size go under them.
 */
export function LibraryFilterRow({
  kind,
  filters,
  countsQuery,
  onChange,
  text,
  onClearText,
  footer,
}: LibraryFilterRowProps) {
  const format = useFormat();
  const counts = useLifecycleCounts(kind);
  const data = counts.data;
  const countOf = (stage: StageChoice): number | undefined =>
    data === undefined ? undefined : stage === "all" ? data.total : data[stage];

  const folders = useQuery({
    queryKey: ["library", "groups", "drive", kind] as const,
    queryFn: () =>
      currentLibraryBackend().sourceGroups?.({
        kind,
        sort: [],
        search: "",
        filters: [],
      }) ?? Promise.resolve([]),
    staleTime: 30_000,
  });
  const folderList = (folders.data ?? []).filter((group) => group.label !== "");

  const problemCount =
    data === undefined ? undefined : data.not_matched + data.uncertain + data.missing;

  const fixCounts = useFixCounts(countsQuery);
  const notScraped = filters.stage === "new";
  const fixOn = filters.fix.length + (notScraped ? 1 : 0);
  const fixCountOf = (entry: FixChoice | "not-scraped"): number | undefined =>
    entry === "not-scraped" ? data?.new : fixCounts.get(entry);
  const toggleFix = (entry: FixChoice | "not-scraped") => {
    if (entry === "not-scraped") {
      onChange({ stage: notScraped ? "all" : "new" });
    } else {
      onChange({
        fix: filters.fix.includes(entry)
          ? filters.fix.filter((choice) => choice !== entry)
          : [...filters.fix, entry],
      });
    }
  };

  return (
    <div
      role="group"
      aria-label={t("library.filters.label")}
      className={cx(
        "flex h-[42px] shrink-0 items-center gap-1.5 px-6",
        "@max-[640px]/pane:h-auto @max-[640px]/pane:flex-wrap @max-[640px]/pane:gap-y-0",
      )}
    >
      <div
        className={cx(
          "flex min-w-0 flex-1 items-center gap-1.5 self-stretch",
          // Sideways scrolling without a scrollbar: the chip cut at the edge says there is more.
          "@max-[860px]/pane:-ml-6 @max-[860px]/pane:overflow-x-auto @max-[860px]/pane:pr-2 @max-[860px]/pane:pl-6",
          "@max-[860px]/pane:[scrollbar-width:none] @max-[860px]/pane:[&::-webkit-scrollbar]:hidden",
          "@max-[640px]/pane:-mr-6 @max-[640px]/pane:h-[42px] @max-[640px]/pane:basis-full @max-[640px]/pane:pr-6",
        )}
      >
        {STAGES.map((stage) => {
          const count = countOf(stage);
          return (
            <Chip
              key={stage}
              active={filters.stage === stage}
              count={count === undefined ? undefined : format.number(count)}
              onClick={() => onChange({ stage })}
            >
              {t(STAGE_LABEL[stage])}
            </Chip>
          );
        })}
        <span aria-hidden="true" className="mx-1.5 h-[18px] w-px bg-line-section" />

        <Menu
          aria-label={t("library.drive.label")}
          trigger={
            <Chip active={filters.drive !== null} trailingIcon={ChevronDown}>
              {t("library.drive.chip", { value: filters.drive ?? t("library.drive.all") })}
            </Chip>
          }
        >
          <MenuRadioGroup
            value={filters.drive ?? ""}
            onValueChange={(value) => onChange({ drive: value === "" ? null : value })}
          >
            <MenuRadioItem value="">{t("library.drive.all")}</MenuRadioItem>
            {folderList.map((group) => (
              <MenuRadioItem key={group.label} value={group.label}>
                {t("library.group.header", { label: group.label, count: group.count })}
              </MenuRadioItem>
            ))}
          </MenuRadioGroup>
        </Menu>

        <Menu
          aria-label={t("library.quality.label")}
          trigger={
            <Chip active={filters.quality !== "any"} trailingIcon={ChevronDown}>
              {t("library.quality.chip", { value: t(QUALITY_LABEL[filters.quality]) })}
            </Chip>
          }
        >
          <MenuRadioGroup
            value={filters.quality}
            onValueChange={(value) => onChange({ quality: value as QualityChoice })}
          >
            {QUALITIES.map((quality) => (
              <MenuRadioItem key={quality} value={quality}>
                {t(QUALITY_LABEL[quality])}
              </MenuRadioItem>
            ))}
          </MenuRadioGroup>
        </Menu>

        <Menu
          aria-label={t("library.problems.label")}
          trigger={
            <Chip
              tone="warn"
              active={filters.problems !== "any"}
              icon={TriangleAlert}
              trailingIcon={ChevronDown}
              data-testid="problems-chip"
            >
              {t("library.problems.chip", { count: problemCount ?? 0 })}
            </Chip>
          }
        >
          <MenuRadioGroup
            value={filters.problems}
            onValueChange={(value) => onChange({ problems: value as ProblemChoice })}
          >
            <MenuRadioItem value="any">{t("library.problems.any")}</MenuRadioItem>
            {PROBLEMS.map((problem) => (
              <MenuRadioItem key={problem} value={problem}>
                {t(PROBLEM_LABEL[problem])}
              </MenuRadioItem>
            ))}
          </MenuRadioGroup>
        </Menu>

        {kind === "movies" && (filters.copies || (data?.copies ?? 0) > 0) && (
          <Chip
            active={filters.copies}
            icon={Copy}
            count={format.number(data?.copies ?? 0)}
            onClick={() => onChange({ copies: !filters.copies })}
            data-testid="copies-chip"
          >
            {t("copies.chip")}
          </Chip>
        )}

        <Menu
          aria-label={t("library.fix.label")}
          trigger={
            <Chip
              active={fixOn > 0}
              icon={Wrench}
              trailingIcon={ChevronDown}
              {...(fixOn > 0 ? { count: format.number(fixOn) } : {})}
              data-testid="needs-fixing-chip"
            >
              {t("library.fix.chip")}
            </Chip>
          }
        >
          {FIX_ENTRIES.map((entry) => {
            const count = fixCountOf(entry);
            return (
              <MenuCheckboxItem
                key={entry}
                checked={entry === "not-scraped" ? notScraped : filters.fix.includes(entry)}
                onCheckedChange={() => toggleFix(entry)}
              >
                {t(FIX_LABEL[entry])}
                {count !== undefined && (
                  <>
                    {" "}
                    <span className="ml-1 text-text-muted tabular-nums">
                      {format.number(count)}
                    </span>
                  </>
                )}
              </MenuCheckboxItem>
            );
          })}
          <MenuSeparator />
          <MenuItem
            disabled={fixOn === 0}
            onSelect={() => onChange({ fix: [], ...(notScraped ? { stage: "all" } : {}) })}
          >
            {t("library.fix.clear")}
          </MenuItem>
        </Menu>

        {text !== "" && (
          <Chip
            active
            trailingIcon={X}
            onClick={onClearText}
            aria-label={t("library.search.clear", { text })}
            className="max-w-[240px]"
          >
            <span className="truncate">{t("library.search.chip", { text })}</span>
          </Chip>
        )}
      </div>
      <span
        className={cx(
          "shrink-0 text-(length:--fs-12) text-text-muted tabular-nums",
          "@max-[640px]/pane:-mt-0.5 @max-[640px]/pane:pb-2",
        )}
        aria-live="polite"
      >
        {footer}
      </span>
    </div>
  );
}
