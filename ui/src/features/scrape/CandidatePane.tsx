import { ScanSearch } from "lucide-react";

import { Button } from "../../components/Button";
import { Kbd } from "../../components/Kbd";
import { PathText } from "../../components/PathText";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { ChangeCard, CandidateDetails } from "./CandidateDetails";
import { CandidateList } from "./CandidateList";
import { CandidateSearchBar } from "./CandidateSearchBar";
import { minutesOf, runtimeText } from "./candidateText";
import type { ScrapeStreamHit } from "./scrapeBackend";
import type { QueueEntry } from "./scrapeQueue";
import { CandidateExtras } from "./CandidateExtras";
import { artworkCountOf, useCandidatePreview } from "./useArtworkCount";
import { useCandidateSearch } from "./useCandidateSearch";

/** The file under the search: its path in full, and what is known of it. */
function EvidenceLine({ entry }: { entry: QueueEntry }) {
  const format = useFormat();
  const facts = [
    entry.sizeBytes === null ? null : format.size(entry.sizeBytes),
    runtimeText(minutesOf(entry.runtimeSeconds)),
    entry.quality,
  ].filter((part): part is string => part !== null && part !== "");
  return (
    <div className="flex items-baseline gap-2.5" aria-label={t("scrapePage.evidence.label")}>
      <PathText
        path={entry.path}
        className="cv-selectable min-w-0 font-mono text-(length:--fs-13) text-text"
      />
      {facts.length > 0 ? (
        <span className="shrink-0 text-(length:--fs-12) text-text-muted">{facts.join(" · ")}</span>
      ) : null}
    </div>
  );
}

interface CandidatePaneProps {
  entry: QueueEntry;
  /** Candidates found already (a review row's, or the search-only pass's). */
  hits: readonly ScrapeStreamHit[];
  onScrape: (hit: ScrapeStreamHit) => void;
  onSkip: () => void;
}

/**
 * The right side of the Scrape page: the file's evidence and the search for the
 * current title, its candidates as they stream in, the chosen one in detail with "What will
 * change", and "Scrape with this" (Enter) or Skip (Down). Nothing is written until Scrape.
 */
export function CandidatePane({ entry, hits, onScrape, onSkip }: CandidatePaneProps) {
  const search = useCandidateSearch({
    itemId: entry.itemId,
    kind: entry.kind,
    title: entry.title,
    year: entry.year,
    seed: hits,
  });
  const { chosen } = search;
  const preview = useCandidatePreview(entry.itemId, chosen);
  const artworkCount = entry.status === "scraped" ? null : artworkCountOf(preview);
  const apply = () => {
    if (chosen !== null) onScrape(chosen);
  };
  const local = { year: entry.year, runtimeMinutes: minutesOf(entry.runtimeSeconds) };
  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-col gap-2.5 border-b border-solid border-line-section px-6 pt-3.5 pb-3">
        <EvidenceLine entry={entry} />
        <CandidateSearchBar search={search} onEnter={apply} />
      </div>
      <div className="flex min-h-0 flex-1">
        <CandidateList
          search={search}
          local={local}
          onApply={apply}
          className="w-[400px] shrink-0 border-r border-solid border-line-section"
        />
        <div className="flex min-w-0 flex-1 flex-col gap-3.5 overflow-y-auto px-6 py-[18px]">
          {chosen === null ? null : (
            <>
              <CandidateDetails hit={chosen} kind={search.kind} itemId={entry.itemId} />
              <CandidateExtras rows={preview} />
              <ChangeCard
                nowName={entry.file}
                hit={chosen}
                kind={search.kind}
                fromNew={entry.status !== "scraped"}
                artworkCount={artworkCount}
                sameMatch={search.current}
              />
            </>
          )}
          <div className="grow" />
          <div className="flex shrink-0 items-center gap-2">
            <Button
              variant="primary"
              size="lg"
              icon={ScanSearch}
              disabled={chosen === null}
              title={t("scrapePage.scrape.tip")}
              onClick={apply}
            >
              {t("scrapePage.scrape")}
            </Button>
            <Kbd>Enter</Kbd>
            <span className="grow" />
            <Button variant="ghost" shortcut="↓" onClick={onSkip}>
              {t("scrapePage.skip")}
            </Button>
          </div>
        </div>
      </div>
    </div>
  );
}
