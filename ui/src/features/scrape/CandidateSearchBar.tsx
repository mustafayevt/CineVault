import { Search } from "lucide-react";
import { useEffect, useRef } from "react";
import type { KeyboardEvent } from "react";

import { Field } from "../../components/Field";
import { SegmentedControl } from "../../components/SegmentedControl";
import { t } from "../../i18n/catalog";
import type { ScrapeKind } from "./scrapeBackend";
import type { CandidateSearch } from "./useCandidateSearch";

interface CandidateSearchBarProps {
  search: CandidateSearch;
  /** The search field takes the focus when the bar appears (a picker page). */
  focusOnOpen?: boolean;
  /** Enter in the field applies the chosen candidate. */
  onEnter: () => void;
  className?: string;
}

/**
 * The search row of the Scrape page and the match picker: the Movie |
 * TV show switch, one field that takes a title or a pasted IMDb or TMDB link or ID, and the year.
 * Typing searches; Up and Down move through the candidates and Enter applies the chosen one.
 */
export function CandidateSearchBar({
  search,
  focusOnOpen = false,
  onEnter,
  className,
}: CandidateSearchBarProps) {
  // A picker page opens with the cursor in the search field; the Scrape page keeps the focus where it was.
  const field = useRef<HTMLInputElement>(null);
  useEffect(() => {
    if (focusOnOpen) field.current?.focus();
  }, [focusOnOpen]);
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      search.move(event.key === "ArrowDown" ? 1 : -1);
    } else if (event.key === "Enter") {
      event.preventDefault();
      onEnter();
    }
  };
  return (
    <div className={className ?? "flex items-center gap-2"}>
      <SegmentedControl<ScrapeKind>
        aria-label={t("scrapePage.kind.label")}
        value={search.kind}
        onChange={search.setKind}
        options={[
          { value: "movie", label: t("scrapePage.kind.movie") },
          { value: "show", label: t("scrapePage.kind.show") },
        ]}
      />
      <Field
        icon={Search}
        height="search"
        type="text"
        role="searchbox"
        autoComplete="off"
        spellCheck={false}
        ref={field}
        aria-label={t("scrapePage.search.label")}
        value={search.text}
        onChange={(event) => search.setText(event.target.value)}
        onKeyDown={onKeyDown}
        className="min-w-0 flex-1"
        trailing={
          <span
            title={t("scrapePage.search.hintTip")}
            className="text-(length:--fs-12) whitespace-nowrap text-text-muted select-none"
          >
            {t("scrapePage.search.hint")}
          </span>
        }
      />
      <Field
        height="search"
        type="text"
        inputMode="numeric"
        maxLength={4}
        autoComplete="off"
        aria-label={t("scrapePage.year.label")}
        placeholder={t("scrapePage.year.label")}
        value={search.isLink ? "" : search.yearText}
        disabled={search.isLink}
        onChange={(event) => search.setYearText(event.target.value.replace(/\D/g, ""))}
        onKeyDown={onKeyDown}
        className="w-[84px] shrink-0"
      />
    </div>
  );
}
