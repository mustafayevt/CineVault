import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import { activeLocale } from "../../i18n/locale";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import { idLinks, runtimeText } from "./candidateText";
import type { ScrapeKind, ScrapeStreamHit } from "./scrapeBackend";
import { hitName, otherLibraryLink } from "./scrapeModel";

/** The language of a title as people say it ("Russian"); the code itself when unknown. */
function languageLabel(code: string | null): string | null {
  if (code === null || code === "") return null;
  try {
    return new Intl.DisplayNames([activeLocale()], { type: "language" }).of(code) ?? code;
  } catch {
    return code;
  }
}

interface CandidateDetailsProps {
  hit: ScrapeStreamHit;
  kind: ScrapeKind;
  /** The library item being scraped, to tell a title that already belongs to another one. */
  itemId: string;
}

/**
 * The chosen candidate: poster, name with year, runtime and language, director, the
 * IDs it carries (each a link to its page) and the plot.
 */
export function CandidateDetails({ hit, kind, itemId }: CandidateDetailsProps) {
  const { candidate } = hit;
  const key = candidate.posterKey;
  const year = candidate.year !== null && candidate.year > 0 ? candidate.year : null;
  const facts = [runtimeText(candidate.runtimeMinutes), languageLabel(candidate.language)].filter(
    (part): part is string => part !== null,
  );
  const links = idLinks(hit, kind);
  const elsewhere = otherLibraryLink(hit, { itemId });
  return (
    <div className="flex flex-col gap-3.5" data-testid="candidate-details">
      <div className="flex gap-[18px]">
        <div className="relative aspect-[2/3] w-[132px] shrink-0 overflow-hidden rounded-(--r-10) shadow-floating">
          {key !== null && isImageKey(key) ? (
            <CachedImage imageKey={key} size="hero" alt="" className="size-full" />
          ) : (
            <PlaceholderPoster title={candidate.title} className="size-full" />
          )}
        </div>
        <div className="flex min-w-0 flex-col gap-1.5">
          <h2 className="m-0 text-(length:--fs-20) leading-[1.15] font-(weight:--fw-700)">
            {candidate.title}
            {year === null ? null : (
              <span className="font-(weight:--fw-500) text-text-muted"> ({year})</span>
            )}
          </h2>
          {facts.length > 0 ? <div className="text-text-2">{facts.join(" · ")}</div> : null}
          {candidate.director === null || candidate.director === "" ? null : (
            <div className="text-text-2">
              {t("scrapePage.details.directedBy", { name: candidate.director })}
            </div>
          )}
          {links.length > 0 ? (
            <div className="cv-selectable text-(length:--fs-12)">
              {links.map((link, index) => (
                <span key={link.label}>
                  {index > 0 ? <span className="text-text-muted"> · </span> : null}
                  {link.url === null ? (
                    <span className="text-text-2">
                      {link.label} {link.value}
                    </span>
                  ) : (
                    <a
                      href={link.url}
                      target="_blank"
                      rel="noreferrer"
                      className="cv-link cursor-pointer"
                    >
                      {link.label} {link.value}
                    </a>
                  )}
                </span>
              ))}
            </div>
          ) : null}
        </div>
      </div>
      <p className="m-0 max-w-[70ch] text-text-2 leading-[1.55]">
        {candidate.overview === null || candidate.overview === ""
          ? t("scrapePage.details.noPlot")
          : candidate.overview}
      </p>
      {elsewhere === null ? null : (
        <p role="note" className="m-0 text-(length:--fs-12) text-warn">
          {t("scrapePage.details.inLibrary", { name: elsewhere.name })}
        </p>
      )}
    </div>
  );
}

interface ChangeCardProps {
  /** What the title is called now: the file name for a New title. */
  nowName: string;
  hit: ScrapeStreamHit;
  kind: ScrapeKind;
  /** The title is New: its stage becomes Scraped. */
  fromNew: boolean;
  /** How many artwork files the choice would write, once known. */
  artworkCount: number | null;
  /** The chosen candidate is the match the title has now. */
  sameMatch?: boolean;
}

/** The stem of a file name: the name without its extension. */
function stemOf(name: string): string {
  return name.replace(/\.[A-Za-z0-9]{2,4}$/, "");
}

/**
 * "What will change" for a title that has no match yet: the title becoming the
 * candidate's, the stage, the files written and that the folder stays until Rename.
 */
export function ChangeCard({
  nowName,
  hit,
  kind,
  fromNew,
  artworkCount,
  sameMatch = false,
}: ChangeCardProps) {
  const nfo = kind === "movie" ? "movie.nfo" : "tvshow.nfo";
  return (
    <section
      aria-label={t("scrapePage.change.heading")}
      className="rounded-(--r-10) border border-solid border-line-section bg-surface-card px-3.5 py-3"
    >
      <SectionLabel className="mb-2">{t("scrapePage.change.heading")}</SectionLabel>
      <dl className="m-0 grid grid-cols-[92px_minmax(0,1fr)] gap-x-2.5 gap-y-1">
        <dt className="text-text-muted">{t("scrapePage.change.title")}</dt>
        <dd className="m-0 flex min-w-0 items-baseline gap-1.5">
          <span title={nowName} className="max-w-[45%] min-w-0 shrink truncate">
            {stemOf(nowName)}
          </span>
          <span className="text-text-muted">→</span>
          <span data-testid="change-title" className="min-w-0 truncate">
            {hitName(hit.candidate)}
          </span>
        </dd>
        <dt className="text-text-muted">{t("scrapePage.change.stage")}</dt>
        <dd className="m-0">
          {fromNew ? (
            <>
              {t("scrapePage.change.stageNew")} <span className="text-text-muted">→</span>{" "}
              {t("scrapePage.change.stageScraped")}
            </>
          ) : (
            t("scrapePage.change.stageScraped")
          )}
        </dd>
        <dt className="text-text-muted">{t("scrapePage.change.files")}</dt>
        <dd className="m-0" data-testid="change-files">
          {artworkCount === null
            ? t(kind === "movie" ? "scrapePage.change.filesMovie" : "scrapePage.change.filesShow")
            : t("scrapePage.change.filesCount", { nfo, count: artworkCount })}
        </dd>
        <dt className="text-text-muted">{t("scrapePage.change.folder")}</dt>
        <dd className="m-0">{t("scrapePage.change.folderStays")}</dd>
      </dl>
      {sameMatch ? (
        <p className="m-0 mt-2 text-(length:--fs-12) text-text-muted">
          {t("scrapePage.change.sameMatch")}
        </p>
      ) : null}
    </section>
  );
}
