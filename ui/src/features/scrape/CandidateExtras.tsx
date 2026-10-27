import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import type { ScrapeFieldDiff } from "./scrapeBackend";

/** The facts shown for a candidate, in this order, when the provider has them. */
const FACT_FIELDS = [
  "original_title",
  "genres",
  "release_date",
  "certification",
  "rating",
  "studio",
  "country",
  "collection",
] as const;

/** How many cast names the line shows before "and N more". */
const CAST_SHOWN = 8;

/** The width-to-height ratio each artwork type is drawn at in the strip. */
const ART_RATIO: Record<string, string> = {
  poster: "2 / 3",
  keyart: "2 / 3",
  characterart: "2 / 3",
  disc: "1 / 1",
  squareart: "1 / 1",
  banner: "54 / 10",
  clearlogo: "16 / 6",
  clearart: "16 / 9",
};

function fieldLabel(field: string): string {
  return t(`scrape.field.${field}` as Parameters<typeof t>[0]);
}

type Filled = ScrapeFieldDiff & { incoming: string };

function filled(row: ScrapeFieldDiff | undefined): row is Filled {
  return row !== undefined && row.incoming !== null && row.incoming.trim() !== "";
}

type WithImage = ScrapeFieldDiff & { incomingImageKey: string };

function withImage(row: ScrapeFieldDiff): row is WithImage {
  return (
    row.group === "artwork" && row.incomingImageKey !== null && isImageKey(row.incomingImageKey)
  );
}

interface CandidateExtrasProps {
  /** The provider's fields for the chosen candidate; `null` while they are still arriving. */
  rows: readonly ScrapeFieldDiff[] | null;
}

/**
 * What else the provider has for the chosen candidate (from the read-only preview): its tagline,
 * facts, cast and the artwork it would bring, so the choice can be judged before scraping.
 */
export function CandidateExtras({ rows }: CandidateExtrasProps) {
  if (rows === null) {
    return (
      <p className="m-0 text-(length:--fs-12) text-text-muted" data-testid="candidate-extras-wait">
        {t("scrapePage.extras.loading")}
      </p>
    );
  }
  const byField = new Map(rows.map((row) => [row.field, row]));
  const tagline = byField.get("tagline");
  const facts = FACT_FIELDS.map((field) => byField.get(field)).filter(filled);
  const cast = byField.get("actors");
  const names = filled(cast)
    ? cast.incoming
        .split(",")
        .map((name) => name.trim())
        .filter((name) => name !== "")
    : [];
  const art = rows.filter(withImage);
  return (
    <div className="flex flex-col gap-4" data-testid="candidate-extras">
      {filled(tagline) ? <p className="m-0 text-text-muted italic">{tagline.incoming}</p> : null}
      {facts.length > 0 ? (
        <dl className="m-0 grid max-w-[880px] grid-cols-[110px_minmax(0,1fr)_110px_minmax(0,1fr)] gap-x-3 gap-y-[9px]">
          {facts.map((row) => (
            <div key={row.field} className="contents">
              <dt className="text-text-muted">{fieldLabel(row.field)}</dt>
              <dd className="cv-selectable m-0 min-w-0 break-words">{row.incoming}</dd>
            </div>
          ))}
        </dl>
      ) : null}
      {names.length > 0 ? (
        <div className="flex flex-col gap-1.5">
          <SectionLabel>{fieldLabel("actors")}</SectionLabel>
          <p className="cv-selectable m-0 max-w-[90ch] text-text-2">
            {names.slice(0, CAST_SHOWN).join(", ")}
            {names.length > CAST_SHOWN
              ? t("scrapePage.extras.more", { count: names.length - CAST_SHOWN })
              : null}
          </p>
        </div>
      ) : null}
      {art.length > 0 ? (
        <div className="flex flex-col gap-2">
          <SectionLabel>{t("scrapePage.extras.artwork")}</SectionLabel>
          <ul className="m-0 flex list-none flex-wrap gap-3 p-0">
            {art.map((row) => (
              <li key={row.field} className="flex flex-col gap-1.5">
                <div
                  className="h-[112px] overflow-hidden rounded-(--r-6) bg-surface-raised"
                  style={{ aspectRatio: ART_RATIO[row.field] ?? "16 / 9" }}
                >
                  <CachedImage
                    imageKey={row.incomingImageKey}
                    size="grid"
                    alt={fieldLabel(row.field)}
                    className="size-full"
                  />
                </div>
                <span className="text-(length:--fs-12) text-text-muted">
                  {fieldLabel(row.field)}
                </span>
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </div>
  );
}
