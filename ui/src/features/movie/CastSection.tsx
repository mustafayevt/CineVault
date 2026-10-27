import { useEffect, useId, useMemo, useRef, useState } from "react";

import type { DetailCreditDto } from "../../bindings";
import { PlaceholderFace } from "../../components/PlaceholderPoster";
import { SectionLabel } from "../../components/SectionLabel";
import { t } from "../../i18n/catalog";
import { Tooltip } from "../../components/Tooltip";
import { DetailImage } from "../detail/DetailImage";
import { CAST_KINDS, castGroups, mergeCrew } from "../detail/castModel";
import { hasPersonPage, openPersonPage } from "./personPage";

/** At most this many people show before "All N". */
const FIRST = 7;
/** One person's width and the gap between two. */
const PERSON_WIDTH = 82;
const PERSON_GAP = 10;

function Person({ person }: { person: DetailCreditDto }) {
  const face = (
    <>
      <div className="relative size-[72px] overflow-hidden rounded-(--r-pill) bg-surface-raised">
        {person.portrait === null ? (
          <PlaceholderFace name={person.name} />
        ) : (
          <DetailImage image={person.portrait} size="grid" className="object-cover" />
        )}
      </div>
      <div className="text-(length:--fs-13) leading-tight font-(weight:--fw-600)">
        {person.name}
      </div>
      {person.role === "" ? null : (
        <div className="-mt-[3px] text-(length:--fs-12) leading-tight text-text-muted">
          {person.role}
        </div>
      )}
    </>
  );
  const label = t("movie.cast.openImdb", { name: person.name });
  return (
    <li className="w-[82px] shrink-0" data-testid="cast-person">
      {hasPersonPage(person) ? (
        <Tooltip content={label}>
          <button
            type="button"
            aria-label={label}
            onClick={() => void openPersonPage(person)}
            className="cv-lift flex w-full cursor-pointer flex-col gap-1.5 rounded-(--r-6) border-0 bg-transparent p-0 text-left text-inherit"
          >
            {face}
          </button>
        </Tooltip>
      ) : (
        <div className="flex flex-col gap-1.5">{face}</div>
      )}
    </li>
  );
}

/** The crew as one line per department: the names, each person once with the jobs joined. */
function Crew({ credits }: { credits: readonly DetailCreditDto[] }) {
  const groups = castGroups(credits).filter((group) => group.key !== "cast");
  if (groups.length === 0) return null;
  return (
    <dl className="m-0 mt-4 grid grid-cols-[110px_minmax(0,1fr)] gap-x-3 gap-y-[9px]">
      {groups.map((group) => (
        <div key={group.key} className="contents">
          <dt className="text-text-muted">{group.label}</dt>
          <dd className="m-0 min-w-0">
            {mergeCrew(group.people, (credit) => (credit.role === "" ? group.label : credit.role))
              .map((person) =>
                person.role === "" ? person.name : `${person.name} (${person.role})`,
              )
              .join(", ")}
          </dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * The cast of the Movie page: the first seven people with photo, name and character; "All N"
 * expands the rest of the cast and the crew in place (`.cv-expand`) and becomes "Show less". N is
 * the number of distinct people, each listed once.
 */
export function CastSection({ credits }: { credits: readonly DetailCreditDto[] }) {
  const [open, setOpen] = useState(false);
  // The first row never wraps: as many people as fit its width, up to seven; the rest wait
  // behind "All N".
  const [fit, setFit] = useState(FIRST);
  const row = useRef<HTMLUListElement>(null);
  const regionId = useId();
  useEffect(() => {
    const element = row.current;
    if (element === null || typeof ResizeObserver === "undefined") return;
    // A few pixels of slack: the last person may lose its side margin but never its photo.
    const measure = () => {
      const width = element.clientWidth;
      if (width <= 0) return;
      const count = Math.floor((width + PERSON_GAP + 8) / (PERSON_WIDTH + PERSON_GAP));
      setFit(Math.min(FIRST, Math.max(1, count)));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, [credits.length]);
  const cast = useMemo(() => credits.filter((credit) => CAST_KINDS.has(credit.kind)), [credits]);
  const total = useMemo(
    () => new Set(credits.map((credit) => credit.name.trim().toLowerCase())).size,
    [credits],
  );
  if (credits.length === 0) return null;
  const rest = cast.slice(fit);
  const expandable = rest.length > 0 || credits.length > cast.length;
  return (
    <section aria-label={t("movie.cast.title")}>
      <SectionLabel
        action={
          expandable ? (
            <button
              type="button"
              aria-expanded={open}
              aria-controls={regionId}
              onClick={() => setOpen((value) => !value)}
              className="cv-link"
            >
              {open ? t("movie.cast.less") : t("movie.cast.all", { count: total })}
            </button>
          ) : undefined
        }
      >
        {t("movie.cast.title")}
      </SectionLabel>
      <ul ref={row} className="m-0 flex list-none gap-x-2.5 overflow-hidden p-0">
        {cast.slice(0, fit).map((person, index) => (
          <Person key={`${person.name}:${index}`} person={person} />
        ))}
      </ul>
      <div id={regionId} className="cv-expand" data-open={open ? "true" : "false"}>
        <div>
          <ul className="m-0 mt-4 flex list-none flex-wrap gap-x-2.5 gap-y-4 p-0">
            {rest.map((person, index) => (
              <Person key={`${person.name}:${fit + index}`} person={person} />
            ))}
          </ul>
          <Crew credits={credits} />
        </div>
      </div>
    </section>
  );
}
