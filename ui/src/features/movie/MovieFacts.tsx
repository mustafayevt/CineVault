import { Link } from "@tanstack/react-router";
import type { ReactNode } from "react";

import { t } from "../../i18n/catalog";
import type { useFormat } from "../../i18n/useFormat";
import { countryName } from "../edit/names";
import { languageName } from "../detail/detailFormat";
import type { MovieDetail } from "../detail/detailBackend";
import { providerIdLabel, providerIdUrl } from "../edit/providerIds";
import { openRatingPage } from "../ratings/ratingPage";

interface Fact {
  id: string;
  label: string;
  value: ReactNode;
}

/** A provider ID as a quiet link that opens the title's page on that provider in the browser. */
function IdLink({ source, id }: { source: string; id: string }) {
  const label = providerIdLabel(source);
  const url = providerIdUrl(source, id, "movie");
  const text = `${label} ${id}`;
  if (url === null) return <span>{text}</span>;
  return (
    <a
      href={url}
      onClick={(event) => {
        event.preventDefault();
        void openRatingPage(url, label);
      }}
      className="cv-link"
    >
      {text}
    </a>
  );
}

/** The facts of a movie in a fixed order, each only when the title has it. */
function movieFacts(detail: MovieDetail, format: ReturnType<typeof useFormat>): Fact[] {
  const facts: Fact[] = [];
  const add = (id: string, label: string, value: ReactNode | null) => {
    if (value !== null && value !== "") facts.push({ id, label, value });
  };
  add(
    "director",
    t("movie.facts.director", { count: detail.directors.length }),
    detail.directors.length === 0 ? null : format.list(detail.directors),
  );
  add(
    "released",
    t("movie.facts.released"),
    detail.release_date === null ? null : format.date(`${detail.release_date}T00:00:00`),
  );
  add(
    "writers",
    t("movie.facts.writers", { count: detail.writers.length }),
    detail.writers.length === 0 ? null : format.list(detail.writers),
  );
  const languages =
    detail.original_language !== ""
      ? [languageName(detail.original_language, format.locale)]
      : detail.languages.map((language) => languageName(language, format.locale));
  add(
    "language",
    t("movie.facts.language"),
    languages.length === 0 ? null : format.list(languages),
  );
  add(
    "studio",
    t("movie.facts.studio", { count: detail.studios.length }),
    detail.studios.length === 0 ? null : format.list(detail.studios),
  );
  add(
    "country",
    t("movie.facts.country", { count: detail.countries.length }),
    detail.countries.length === 0 ? null : format.list(detail.countries.map(countryName)),
  );
  add(
    "collection",
    t("movie.facts.collection"),
    detail.collection === null ? null : (
      <Link to="/library" search={{ type: "collections" }} className="cv-link">
        {detail.collection.name}
      </Link>
    ),
  );
  add(
    "ids",
    t("movie.facts.ids"),
    detail.ids.length === 0 ? null : (
      <span className="flex flex-wrap gap-x-1">
        {detail.ids.map((id, index) => (
          <span key={id.provider}>
            <IdLink source={id.provider} id={id.value} />
            {index === detail.ids.length - 1 ? "" : " ·"}
          </span>
        ))}
      </span>
    ),
  );
  return facts;
}

/**
 * The facts of the Movie page: a four-column list (110px label, value, 110px label, value) with
 * 9px between rows. A title without a value for a fact leaves the fact out.
 */
export function MovieFacts({
  detail,
  format,
}: {
  detail: MovieDetail;
  format: ReturnType<typeof useFormat>;
}) {
  const facts = movieFacts(detail, format);
  if (facts.length === 0) return null;
  return (
    <dl className="m-0 grid grid-cols-[110px_minmax(0,1fr)_110px_minmax(0,1fr)] gap-x-3 gap-y-[9px]">
      {facts.map((fact) => (
        <div key={fact.id} className="contents">
          <dt className="text-text-muted">{fact.label}</dt>
          <dd data-fact={fact.id} className="m-0 min-w-0 break-words">
            {fact.value}
          </dd>
        </div>
      ))}
    </dl>
  );
}
