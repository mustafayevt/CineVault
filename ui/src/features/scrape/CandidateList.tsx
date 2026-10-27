import type { ReactNode } from "react";
import { useEffect, useRef } from "react";

import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { ScoreBadge } from "../../components/ScoreBadge";
import { Skeleton } from "../../components/Skeleton";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import type { ProviderStatus } from "../workspaces/providerEvents";
import { providerName } from "../workspaces/providerNames";
import { useOpenProviderKey } from "../workspaces/useOpenProviderKey";
import { metaLine, providersOf, whyLine } from "./candidateText";
import type { LocalFacts } from "./candidateText";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { percentOf, scoreTone } from "./scrapeQueue";
import type { CandidateSearch } from "./useCandidateSearch";

/** The small heading over a group of candidates (the section label with the list's own padding). */
function ListLabel({ children }: { children: ReactNode }) {
  return (
    <p className="m-0 px-3 pt-1 pb-1.5 text-(length:--fs-11) leading-[1.3] font-(weight:--fw-600) tracking-[0.06em] text-text-muted uppercase">
      {children}
    </p>
  );
}

/** The poster of a candidate at the size of a list row. */
function CandidatePoster({ hit, className }: { hit: ScrapeStreamHit; className?: string }) {
  const key = hit.candidate.posterKey;
  return (
    <span
      className={cx("relative h-[60px] w-10 shrink-0 overflow-hidden rounded-(--r-4)", className)}
    >
      {key !== null && isImageKey(key) ? (
        <CachedImage imageKey={key} size="list" alt="" className="size-full" />
      ) : (
        <PlaceholderPoster title={hit.candidate.title} className="size-full" />
      )}
    </span>
  );
}

interface CandidateRowProps {
  hit: ScrapeStreamHit;
  local: LocalFacts;
  chosen: boolean;
  /** Marks the title's current match. */
  current?: boolean;
  /** The line under the title in place of the explanation of the score. */
  note?: string;
  onChoose: () => void;
  onApply: () => void;
}

/** One candidate: poster, name with year, score, the runtime and director, and why. */
function CandidateRow({
  hit,
  local,
  chosen,
  current = false,
  note,
  onChoose,
  onApply,
}: CandidateRowProps) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (chosen) ref.current?.scrollIntoView({ block: "nearest" });
  }, [chosen]);
  const { candidate } = hit;
  const percent = percentOf(candidate.confidence);
  const year = candidate.year !== null && candidate.year > 0 ? candidate.year : null;
  return (
    // The list holds the focus and points at the chosen row (aria-activedescendant).
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus
    <div
      ref={ref}
      id={`candidate-${hit.id}`}
      role="option"
      aria-selected={chosen}
      data-candidate={hit.id}
      onClick={onChoose}
      onDoubleClick={onApply}
      className={cx(
        "cv-hoverable flex cursor-pointer gap-3 rounded-(--r-6) px-3 py-2.5",
        chosen
          ? "bg-selected-row shadow-[inset_2px_0_0_var(--gold)]"
          : "hover:bg-surface-card active:bg-surface-raised",
      )}
    >
      <CandidatePoster hit={hit} />
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <div className="flex items-center gap-2">
          <span className="min-w-0 flex-1 truncate font-(weight:--fw-600)">
            {candidate.title}
            {year === null ? null : <span className="font-normal text-text-muted"> ({year})</span>}
          </span>
          {current ? <Tag>{t("matchPicker.currentTag")}</Tag> : null}
          {candidate.confidence === null ? null : (
            <ScoreBadge
              value={t("scrapePage.candidates.score", { percent })}
              source={t("scrapePage.row.topMatch")}
              tone={scoreTone(percent)}
            />
          )}
        </div>
        <div className="truncate text-(length:--fs-12) text-text-2">{metaLine(candidate)}</div>
        <div className="text-(length:--fs-12) text-text-muted">
          {note ?? whyLine(candidate, local)}
        </div>
      </div>
    </div>
  );
}

/** The providers that failed or wait for a key, one quiet line each with what to do about it. */
export function ProviderNotes({
  providers,
  names,
  onRetry,
}: {
  providers: readonly ProviderStatus[];
  names: Readonly<Record<string, string>>;
  onRetry: (provider: string) => void;
}) {
  const openKey = useOpenProviderKey();
  const notes = providers.filter((status) =>
    ["failed", "rejected", "needs-key", "unavailable"].includes(status.state),
  );
  if (notes.length === 0) return null;
  const link = "cv-link cursor-pointer text-(length:--fs-12)";
  return (
    <ul className="m-0 flex list-none flex-col gap-1 px-3 pt-2 pb-1 text-(length:--fs-12) text-text-muted">
      {notes.map((status) => {
        const name = names[status.provider] ?? providerName(status.provider);
        return (
          <li key={status.provider} data-provider={status.provider} data-state={status.state}>
            <span className={status.state === "failed" ? "text-warn" : undefined}>
              {status.state === "failed"
                ? t("scrapePage.provider.failed", { provider: name })
                : status.state === "rejected"
                  ? t("scrapePage.provider.rejected", { provider: name })
                  : status.state === "needs-key"
                    ? t("scrapePage.provider.needsKey", { provider: name })
                    : t("scrapePage.provider.unavailable", { provider: name })}
            </span>{" "}
            {status.state === "failed" ? (
              <button
                type="button"
                className={link}
                aria-label={t("scrapePage.provider.retryLabel", { provider: name })}
                onClick={() => onRetry(status.provider)}
              >
                {t("scrapePage.provider.retry")}
              </button>
            ) : status.state === "rejected" || status.state === "needs-key" ? (
              <button
                type="button"
                className={link}
                aria-label={t("scrapePage.provider.setUpLabel", { provider: name })}
                onClick={() => openKey(status.provider)}
              >
                {t("scrapePage.provider.setUp")}
              </button>
            ) : null}
          </li>
        );
      })}
    </ul>
  );
}

interface CandidateListProps {
  search: CandidateSearch;
  local: LocalFacts;
  onApply: () => void;
  /** The line under the current match, in place of the explanation of a score. */
  currentNote?: string;
  className?: string;
}

/**
 * The candidates of a search: a heading with the count and the providers that
 * answered, the rows as they arrive, a skeleton while nothing has, the reason when nothing is
 * found, and one line per provider that failed. Up and Down move the choice and Enter applies it.
 */
export function CandidateList({
  search,
  local,
  onApply,
  currentNote,
  className,
}: CandidateListProps) {
  const { entries, stream, pinned } = search;
  const listed = entries.filter((hit) => hit.id !== pinned?.id);
  const waiting = stream.searching && entries.length === 0;
  return (
    <div className={cx("flex min-h-0 flex-col gap-0.5 overflow-y-auto px-6 py-3", className)}>
      <div
        role="listbox"
        onKeyDown={(event) => {
          if (event.key === "ArrowDown" || event.key === "ArrowUp") {
            event.preventDefault();
            search.move(event.key === "ArrowDown" ? 1 : -1);
          } else if (event.key === "Enter") {
            event.preventDefault();
            onApply();
          }
        }}
        aria-label={t("scrapePage.candidates.label")}
        aria-activedescendant={search.chosen === null ? undefined : `candidate-${search.chosen.id}`}
        tabIndex={0}
        className="flex flex-col gap-0.5 outline-none"
      >
        {pinned === null ? null : (
          <>
            <ListLabel>{t("matchPicker.current")}</ListLabel>
            <CandidateRow
              hit={pinned}
              local={local}
              chosen={search.chosen?.id === pinned.id}
              current
              {...(currentNote === undefined ? {} : { note: currentNote })}
              onChoose={() => search.choose(pinned)}
              onApply={onApply}
            />
          </>
        )}
        {listed.length > 0 ? (
          <ListLabel>
            {pinned === null
              ? t("scrapePage.candidates.heading", {
                  count: listed.length,
                  providers: providersOf(search),
                })
              : t("matchPicker.others", { count: listed.length, providers: providersOf(search) })}
          </ListLabel>
        ) : null}
        {listed.map((hit) => (
          <CandidateRow
            key={hit.id}
            hit={hit}
            local={local}
            chosen={search.chosen?.id === hit.id}
            onChoose={() => search.choose(hit)}
            onApply={onApply}
          />
        ))}
      </div>
      {waiting ? (
        <div aria-busy="true" className="flex flex-col gap-2 pt-1">
          {[0, 1, 2].map((n) => (
            <Skeleton key={n} shape="line" width="100%" height="60px" />
          ))}
        </div>
      ) : null}
      {!stream.searching && listed.length === 0 && search.query !== "" ? (
        <p role="status" className="m-0 px-3 py-2 text-text-muted">
          {search.nothingFound ?? t("scrapePage.candidates.none")}
        </p>
      ) : null}
      <ProviderNotes
        providers={stream.providers}
        names={search.providerNames}
        onRetry={stream.retry}
      />
    </div>
  );
}
