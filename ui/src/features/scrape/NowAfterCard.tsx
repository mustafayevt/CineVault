import { TriangleAlert } from "lucide-react";
import type { ReactNode } from "react";

import { Icon } from "../../components/Icon";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { runtimeGap, runtimeText } from "./candidateText";
import type { ScrapeStreamHit } from "./scrapeBackend";
import { hitName } from "./scrapeModel";
import { subjectName } from "./titleSubject";
import type { TitleSubject } from "./titleSubject";

function Row({
  label,
  now,
  after,
  warn = false,
}: {
  label: string;
  now: ReactNode;
  after: ReactNode;
  warn?: boolean;
}) {
  return (
    <>
      <dt className="text-text-muted">{label}</dt>
      <dd className="m-0 min-w-0 truncate">{now}</dd>
      <dd className={cx("m-0 min-w-0 truncate", warn && "text-warn")}>{after}</dd>
    </>
  );
}

interface NowAfterCardProps {
  subject: TitleSubject;
  hit: ScrapeStreamHit;
  /** The chosen candidate is the match the title has now. */
  sameMatch: boolean;
}

/**
 * "What will change" of the match picker: the title as it is now beside what
 * it becomes with the chosen candidate: name, director, runtime, NFO and artwork, folder. A runtime
 * that differs from the file's is marked and named in the card's header.
 */
export function NowAfterCard({ subject, hit, sameMatch }: NowAfterCardProps) {
  const { candidate } = hit;
  const fileMinutes =
    subject.durationSeconds === null ? null : Math.round(subject.durationSeconds / 60);
  const gap = sameMatch ? null : runtimeGap(fileMinutes, candidate.runtimeMinutes);
  const nowDirector = subject.directors[0] ?? "";
  const afterDirector = candidate.director ?? "";
  const unknown = t("matchPicker.compare.unknown");
  return (
    <section
      aria-label={t("scrapePage.change.heading")}
      className={cx(
        "flex flex-col gap-2 rounded-(--r-10) border border-solid bg-surface-card px-3.5 py-3",
        gap === null
          ? "border-line-section"
          : "border-[color-mix(in_srgb,var(--warn)_35%,transparent)]",
      )}
    >
      <SectionLabel
        className="mb-0"
        action={
          gap === null ? undefined : (
            <span
              role="status"
              className="inline-flex items-center gap-1.5 text-(length:--fs-12) text-warn"
            >
              <Icon icon={TriangleAlert} size={12} />
              {t("matchPicker.compare.runtimeWarning", { minutes: gap })}
            </span>
          )
        }
      >
        {t("scrapePage.change.heading")}
      </SectionLabel>
      {sameMatch ? (
        <p className="m-0 text-text-2">{t("scrapePage.change.sameMatch")}</p>
      ) : (
        <dl className="m-0 grid grid-cols-[92px_minmax(0,1fr)_minmax(0,1fr)] gap-x-2.5 gap-y-1">
          <dt />
          <dd className="m-0 text-(length:--fs-12) text-text-muted">
            {t("matchPicker.compare.now")}
          </dd>
          <dd className="m-0 text-(length:--fs-12) text-text-muted">
            {t("matchPicker.compare.after")}
          </dd>
          <Row
            label={t("matchPicker.compare.title")}
            now={subjectName(subject)}
            after={hitName(candidate)}
          />
          {nowDirector === "" && afterDirector === "" ? null : (
            <Row
              label={t("matchPicker.compare.director")}
              now={nowDirector || unknown}
              after={afterDirector || unknown}
            />
          )}
          <Row
            label={t("matchPicker.compare.runtime")}
            now={runtimeText(fileMinutes ?? subject.runtimeMinutes) ?? unknown}
            after={runtimeText(candidate.runtimeMinutes) ?? unknown}
            warn={gap !== null}
          />
          <Row
            label={t("matchPicker.compare.files")}
            now={
              subject.metadataFiles > 0
                ? t("matchPicker.compare.filesNow", { count: subject.metadataFiles })
                : unknown
            }
            after={
              subject.metadataFiles > 0
                ? t("matchPicker.compare.filesAfter")
                : t(
                    subject.kind === "movie"
                      ? "scrapePage.change.filesMovie"
                      : "scrapePage.change.filesShow",
                  )
            }
          />
          <Row
            label={t("matchPicker.compare.folder")}
            now={
              <span className="cv-selectable font-mono text-(length:--fs-12)">
                {leaf(subject.folder)}
              </span>
            }
            after={
              <>
                <span className="cv-selectable font-mono text-(length:--fs-12)">
                  {hitName(candidate)}
                </span>{" "}
                <span className="text-text-muted">{t("matchPicker.compare.folderLater")}</span>
              </>
            }
          />
        </dl>
      )}
    </section>
  );
}

function leaf(path: string): string {
  return (
    path
      .split(/[\\/]/)
      .filter((part) => part !== "")
      .pop() ?? path
  );
}
