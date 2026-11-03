import { Link, useNavigate } from "@tanstack/react-router";
import { Check, ChevronDown, Search, Sparkles } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { CachedImage } from "../images/CachedImage";
import { copyFacts, missingText, seasonsText } from "./ownedCopy";
import type { LibraryCopy } from "./ownedCopy";
import type { TitleDetails } from "./torrentBackend";
import { titleName } from "./torrentModel";

interface YouHaveThisProps {
  details: TitleDetails;
  /** Folded into one line ("Search anyway"). */
  folded: boolean;
  onFold: (folded: boolean) => void;
  /** Switches on "Only better than mine" (a movie) or "Fills a gap" (a show). */
  onLook: () => void;
}

function CopyLine({ copy }: { copy: LibraryCopy }) {
  const format = useFormat();
  const facts = copyFacts(copy);
  const languages = copy.languages.map((code) => code.toUpperCase()).join(" · ");
  return (
    <li className="flex min-w-0 items-baseline gap-2 text-(length:--fs-13)">
      <span className="truncate text-text">
        {facts.length === 0 ? t("torrentsPage.have.noInfo") : facts.join(" · ")}
      </span>
      {(copy.size ?? 0) > 0 ? (
        <span className="shrink-0 text-text-2 tabular-nums">{format.size(copy.size ?? 0)}</span>
      ) : null}
      {languages === "" ? null : (
        <span className="shrink-0 text-(length:--fs-12) text-text-muted">{languages}</span>
      )}
    </li>
  );
}

/** "You have S01–S03 · missing S04E05–E08". */
function showSummary(details: TitleDetails): string {
  const held = t("torrentsPage.have.show", { held: seasonsText(details.heldEpisodes) });
  const missing =
    details.missingEpisodes.length === 0
      ? t("torrentsPage.have.complete")
      : t("torrentsPage.have.missing", { episodes: missingText(details.missingEpisodes) });
  return `${held} · ${missing}`;
}

/**
 * "You have this" (spec `torrent-search` "Same, better or worse than yours"): above the results
 * of a title the library holds, the poster and the title linking to its page, then each copy of
 * a movie (best first, the compared one) or the seasons a show has and the episodes it misses,
 * with Open in library, Look for a better copy or Find missing episodes, and Search anyway,
 * which folds it into one line.
 */
export function YouHaveThis({ details, folded, onFold, onLook }: YouHaveThisProps) {
  const navigate = useNavigate();
  const format = useFormat();
  const link = details.inLibrary;
  if (link === null) return null;
  const show = details.kind === "show";
  const name = titleName(details);
  const open = () =>
    void (show
      ? navigate({ to: "/tv/$showId", params: { showId: link.itemId } })
      : navigate({ to: "/movies/$movieId", params: { movieId: link.itemId } }));
  const title = show ? (
    <Link to="/tv/$showId" params={{ showId: link.itemId }} className="cv-link">
      {name}
    </Link>
  ) : (
    <Link to="/movies/$movieId" params={{ movieId: link.itemId }} className="cv-link">
      {name}
    </Link>
  );
  const first = details.libraryCopies[0];

  if (folded) {
    const summary = show
      ? showSummary(details)
      : first === undefined
        ? ""
        : [
            ...copyFacts(first).slice(0, 2),
            (first.size ?? 0) > 0 ? format.size(first.size ?? 0) : "",
          ]
            .filter(Boolean)
            .join(" · ");
    return (
      <div
        data-testid="you-have-this"
        data-folded="true"
        className="flex h-7 min-w-0 items-center gap-2 text-(length:--fs-13) text-text-2"
      >
        <Icon icon={Check} className="text-ok" />
        <span className="truncate">
          {t("torrentsPage.have.head")}: {title}
          {summary === "" ? "" : ` · ${summary}`}
        </span>
        <IconButton
          aria-label={t("torrentsPage.have.unfold")}
          icon={ChevronDown}
          size="sm"
          onClick={() => onFold(false)}
        />
      </div>
    );
  }

  return (
    <section
      aria-label={t("torrentsPage.have.region")}
      data-testid="you-have-this"
      className="flex min-w-0 items-start gap-3.5 rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-3 @max-[640px]/pane:flex-wrap"
    >
      <div className="relative aspect-[2/3] w-[48px] shrink-0 overflow-hidden rounded-(--r-6) bg-surface-active">
        {details.posterKey === null ? (
          <PlaceholderPoster title={name} className="size-full" />
        ) : (
          <CachedImage imageKey={details.posterKey} size="list" alt="" className="size-full" />
        )}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <p className="m-0 flex min-w-0 items-center gap-1.5 text-(length:--fs-12) font-(weight:--fw-600) text-ok">
          <Icon icon={Check} size={14} />
          {t("torrentsPage.have.head")}
          {!show && details.libraryCopies.length > 1 ? (
            <span className="font-(weight:--fw-500) text-text-muted">
              · {t("torrentsPage.have.copies", { count: details.libraryCopies.length })}
            </span>
          ) : null}
        </p>
        <p className="m-0 truncate text-(length:--fs-14) font-(weight:--fw-600) text-text">
          {title}
        </p>
        {show ? (
          <p className="m-0 text-(length:--fs-13) text-text-2">{showSummary(details)}</p>
        ) : (
          <>
            <ul className="m-0 flex list-none flex-col gap-0.5 p-0">
              {details.libraryCopies.map((copy) => (
                <CopyLine key={copy.itemId} copy={copy} />
              ))}
            </ul>
            {details.libraryCopies.length > 1 ? (
              <p className="m-0 text-(length:--fs-12) text-text-muted">
                {t("torrentsPage.have.first")}
              </p>
            ) : null}
          </>
        )}
      </div>
      <div className="flex max-w-full min-w-0 flex-wrap items-center gap-1.5 self-center @min-[641px]/pane:shrink-0">
        <Button size="sm" onClick={open}>
          {t("torrentsPage.have.open")}
        </Button>
        <Button size="sm" icon={Sparkles} onClick={onLook}>
          {t(show ? "torrentsPage.have.gaps" : "torrentsPage.have.better")}
        </Button>
        <Button size="sm" variant="ghost" icon={Search} onClick={() => onFold(true)}>
          {t("torrentsPage.have.anyway")}
        </Button>
      </div>
    </section>
  );
}
