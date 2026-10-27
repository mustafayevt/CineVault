import { useQuery } from "@tanstack/react-query";
import { ChevronDown, TriangleAlert, X } from "lucide-react";

import { Chip } from "../../components/Chip";
import { Menu, MenuRadioGroup, MenuRadioItem } from "../../components/Menu";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useLifecycleCounts } from "../lifecycle/useLifecycleCounts";
import { currentLibraryBackend } from "./libraryBackend";
import type { LibraryKind } from "./libraryBackend";
import type { LibraryFilters, ProblemChoice, QualityChoice, StageChoice } from "./libraryStore";

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

interface LibraryFilterRowProps {
  kind: Exclude<LibraryKind, "collections">;
  filters: LibraryFilters;
  onChange: (change: Partial<LibraryFilters>) => void;
  /** The typed search of the title bar (`?q=`); shown as a chip that clears it. */
  text: string;
  onClearText: () => void;
  /** "181 movies · 7.06 TB". */
  footer: string;
}

/**
 * The filter row: the stage chips All, New, Scraped and Renamed with
 * their counts, then the Drive, Quality and Problems dropdown chips, the typed search as a chip,
 * and at the right the count and size of what is listed. 42px high on the 24px content inset, chips
 * 24px.
 */
export function LibraryFilterRow({
  kind,
  filters,
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

  return (
    <div
      role="group"
      aria-label={t("library.filters.label")}
      className="flex h-[42px] shrink-0 items-center gap-1.5 px-6"
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

      <span className="grow" />
      <span className="text-(length:--fs-12) text-text-muted tabular-nums" aria-live="polite">
        {footer}
      </span>
    </div>
  );
}
