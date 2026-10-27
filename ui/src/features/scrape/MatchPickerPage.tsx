import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { ScanSearch } from "lucide-react";
import { useMemo } from "react";

import { Button } from "../../components/Button";
import { Kbd } from "../../components/Kbd";
import { PathText } from "../../components/PathText";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import { useSetting } from "../settings/settingsStore";
import { useEscapeLayer } from "../shell/escStack";
import { CandidateDetails } from "./CandidateDetails";
import { CandidateExtras } from "./CandidateExtras";
import { CandidateList } from "./CandidateList";
import { CandidateSearchBar } from "./CandidateSearchBar";
import "../title/title.css";
import { minutesOf, runtimeText } from "./candidateText";
import { NowAfterCard } from "./NowAfterCard";
import { currentReviewBackend } from "./reviewBackend";
import { scrapeAndReport } from "./scrapeApply";
import { currentScrapeBackend } from "./scrapeBackend";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { currentIdsOf, subjectName, useTitleSubject } from "./titleSubject";
import type { TitleSubject } from "./titleSubject";
import { useCandidatePreview } from "./useArtworkCount";
import { useCandidateSearch } from "./useCandidateSearch";
import { usePickerClose } from "./usePickerClose";
import { REVIEW_QUEUE_KEY } from "./useScrapeQueue";

/** A soft band behind the top of the chosen candidate: its poster, blurred, on a flat ground. */
function Backdrop({ hit }: { hit: ScrapeStreamHit }) {
  const key = hit.candidate.posterKey;
  return (
    <div
      aria-hidden="true"
      className="relative h-[150px] shrink-0 overflow-hidden bg-surface-window"
    >
      {key !== null && isImageKey(key) ? (
        <CachedImage
          imageKey={key}
          size="hero"
          alt=""
          className="size-full scale-110 opacity-30 blur-2xl"
        />
      ) : null}
      {/* The band melts into the ground instead of ending in a hard edge. */}
      <div className="cv-title-backdrop-fade" />
    </div>
  );
}

interface MatchPickerProps {
  subject: TitleSubject;
}

/** The picker for one title once it has loaded. */
function MatchPicker({ subject }: MatchPickerProps) {
  const queryClient = useQueryClient();
  const navigate = useNavigate();
  const format = useFormat();
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const language = useSetting<string>("metadata.language");

  // A title that waits in the review queue is resolved by the scrape.
  const review = useQuery({
    queryKey: REVIEW_QUEUE_KEY,
    queryFn: () => currentReviewBackend().list(),
  });
  const reviewId = review.data?.find((item) => item.itemId === subject.id)?.id ?? null;

  const currentIds = useMemo(() => currentIdsOf(subject), [subject]);
  // The title it is linked to, scored like the other candidates; the search's own entry for it
  // takes its place once the providers answer.
  const currentMatch = useQuery({
    queryKey: ["scrape-current-match", subject.id, language],
    queryFn: () => currentScrapeBackend().currentMatch(subject.id, language),
    enabled: currentIds.length > 0,
    staleTime: 60_000,
  });
  const currentHit = currentMatch.data ?? null;
  const search = useCandidateSearch({
    itemId: subject.id,
    kind: subject.kind,
    title: subject.title,
    year: subject.year > 0 ? subject.year : null,
    currentIds,
    currentHit,
  });
  const { chosen, pinned } = search;
  const preview = useCandidatePreview(subject.id, chosen);
  const sameMatch = search.current;
  const scoreless = pinned !== null && pinned.candidate.confidence === null;

  const apply = () => {
    if (chosen === null) return;
    const ref = {
      itemId: subject.id,
      kind: search.kind,
      title: subject.title,
      year: subject.year > 0 ? subject.year : null,
      reviewId,
    };
    // The picker is done at once and the page shows the new match; a refusal brings it back.
    close();
    void scrapeAndReport(queryClient, ref, chosen, language, () => {
      void navigate({ to: "/scrape/match/$itemId", params: { itemId: subject.id } });
    });
  };

  const fileMinutes = minutesOf(subject.durationSeconds);
  const facts = [
    subject.fileBytes === null ? null : format.size(subject.fileBytes),
    runtimeText(fileMinutes),
  ].filter((part): part is string => part !== null);
  const path =
    subject.fileName === ""
      ? subject.folder
      : `${subject.folder}${subject.folder.includes("\\") ? "\\" : "/"}${subject.fileName}`;
  const key = subject.poster?.key ?? null;
  const name = subjectName(subject);

  return (
    <section
      aria-label={t("matchPicker.title", { name })}
      className="relative flex h-full min-h-0 flex-col"
    >
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-6 bottom-0 left-6 h-px bg-line-section"
        />
        <span className="relative h-[39px] w-[26px] shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active">
          {key !== null && isImageKey(key) ? (
            <CachedImage imageKey={key} size="list" alt="" className="size-full" />
          ) : (
            <PlaceholderPoster title={subject.title} className="size-full" />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="m-0 truncate text-(length:--fs-16) leading-[1.3] font-(weight:--fw-700)">
            {t("matchPicker.title", { name })}
          </h1>
          <div className="flex items-baseline gap-2 text-(length:--fs-12) text-text-muted">
            <PathText path={path} className="cv-selectable min-w-0 font-mono" />
            {facts.length > 0 ? <span className="shrink-0">{facts.join(" · ")}</span> : null}
          </div>
        </div>
        <Button variant="ghost" shortcut="Esc" onClick={close}>
          {t("matchPicker.cancel")}
        </Button>
      </div>
      <CandidateSearchBar
        search={search}
        focusOnOpen
        onEnter={apply}
        className="flex shrink-0 items-center gap-2 border-b border-solid border-line-section px-6 py-3.5"
      />
      <div className="flex min-h-0 flex-1">
        <CandidateList
          search={search}
          local={{ year: subject.year > 0 ? subject.year : null, runtimeMinutes: fileMinutes }}
          onApply={apply}
          {...(scoreless ? { currentNote: t("matchPicker.currentUnknown") } : {})}
          className="w-[470px] shrink-0 border-r border-solid border-line-section"
        />
        <div className="flex min-w-0 flex-1 flex-col overflow-y-auto">
          {chosen === null ? null : (
            <>
              <Backdrop hit={chosen} />
              <div className="relative -mt-24 flex min-h-0 flex-1 flex-col gap-3.5 px-6 pb-5">
                <CandidateDetails hit={chosen} kind={search.kind} itemId={subject.id} />
                <CandidateExtras rows={preview} />
                <NowAfterCard subject={subject} hit={chosen} sameMatch={sameMatch} />
                <div className="grow" />
                <div className="flex shrink-0 items-center gap-2">
                  <Button
                    variant="primary"
                    size="lg"
                    icon={ScanSearch}
                    title={t("scrapePage.scrape.tip")}
                    onClick={apply}
                  >
                    {t("scrapePage.scrape")}
                  </Button>
                  <Kbd>Enter</Kbd>
                  <span className="grow" />
                  <Button variant="ghost" onClick={close}>
                    {t("matchPicker.cancel")}
                  </Button>
                </div>
              </div>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

/** The route `/scrape/match/$itemId`. */
export function MatchPickerPage() {
  const { itemId } = useParams({ strict: false }) as { itemId?: string };
  const id = itemId ?? "";
  const subject = useTitleSubject(id);
  if (subject.data !== undefined) return <MatchPicker key={id} subject={subject.data} />;
  if (subject.isError) {
    return (
      <section className="flex flex-col items-start gap-3 px-6 pt-6">
        <h1 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
          {t("matchPicker.loadFailed")}
        </h1>
        <Link to="/library" className="cv-link">
          {t("matchPicker.back")}
        </Link>
      </section>
    );
  }
  return (
    <section aria-busy="true" className="flex flex-col gap-3 px-6 pt-4">
      <Skeleton shape="line" width="320px" />
      <Skeleton shape="line" width="100%" height="34px" />
    </section>
  );
}
