import { Link, useNavigate, useSearch } from "@tanstack/react-router";
import { Check, Search, WifiOff, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Chip } from "../../components/Chip";
import { Field } from "../../components/Field";
import { IconButton } from "../../components/IconButton";
import { Icon } from "../../components/Icon";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import type { QualityTier } from "../../components/QualityBadge";
import { ReleaseTable } from "./ReleaseTable";
import type { OwnedCopy } from "./ReleaseTable";
import {
  TIERS,
  sortReleases,
  sourceText,
  tierCounts,
  tierOf,
  useRememberedSort,
} from "./releaseSort";
import { TitleLine } from "./TitleLine";
import { TorrentNotice } from "./TorrentNotice";
import { useTitleDetails } from "./titleQueries";
import type { ReleaseSearchTarget, TitleHit } from "./torrentBackend";
import { noSourceAnswered, titleName } from "./torrentModel";
import { parseTitleRef, scopeOf, titleParam } from "./torrentsSearch";
import type { TitleRef, TorrentsSearch } from "./torrentsSearch";
import { useOnline } from "./useOnline";
import { useReleaseSearch } from "./useReleaseSearch";
import { useTitleMatch } from "./useTitleMatch";
import { useTorrentNotice } from "./useTorrentNotice";

type Chosen = "all" | QualityTier;

/** "Dolby Vision" for the short HDR word the backend sends. */
function hdrWord(hdr: string | null): string {
  return hdr === "DV" ? "Dolby Vision" : (hdr ?? "");
}

/** The release search a confirmed title and the route's scope ask for. */
function targetOf(
  ref: TitleRef,
  search: TorrentsSearch,
  routed: TitleRef | null,
): ReleaseSearchTarget {
  if (ref.kind === "movie") return { kind: "movie", tmdbId: ref.tmdbId };
  const scope = routed?.tmdbId === ref.tmdbId ? scopeOf(search) : { kind: "all" as const };
  return {
    kind: "show",
    tmdbId: ref.tmdbId,
    ...(scope.kind === "all" ? {} : { season: scope.season }),
    ...(scope.kind === "episode" ? { episode: scope.episode } : {}),
  };
}

function Notice() {
  return (
    <section
      aria-label={t("torrentsPage.region")}
      data-testid="torrents-page"
      className="flex h-full min-h-0 flex-col gap-4 px-6 pt-4"
    >
      <h1 className="sr-only">{t("torrentsPage.region")}</h1>
      <TorrentNotice />
    </section>
  );
}

/** A centred sentence in the space of the results. */
function Message({ children, role }: { children: React.ReactNode; role?: "status" | "alert" }) {
  return (
    <div
      {...(role === undefined ? {} : { role })}
      className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-text-2"
    >
      {children}
    </div>
  );
}

function Rows() {
  return (
    <div aria-busy="true" className="flex flex-col px-6 pt-1">
      {[0, 1, 2, 3, 4, 5, 6, 7].map((n) => (
        <Skeleton key={n} shape="row" />
      ))}
    </div>
  );
}

/**
 * `/torrents` (spec `torrent-search`): one search field. After a 400 ms pause in
 * typing the best matching movie or show is named above the results (with a menu to pick another)
 * and its releases list as the sources answer, with resolution chips and counts, a sortable table,
 * and a hint against the copy the library already holds. The route carries a confirmed title
 * (`?title=movie:123`, from a collection or a title page) or the typed text (`?q=`).
 */
function TorrentsContent() {
  const search = useSearch({ strict: false }) as TorrentsSearch;
  const navigate = useNavigate();
  const format = useFormat();
  const online = useOnline();
  const routed = useMemo(() => parseTitleRef(search.title), [search.title]);

  const field = useRef<HTMLInputElement>(null);
  // Opening the page puts the cursor in the field.
  useEffect(() => field.current?.focus(), []);
  const [text, setText] = useState(search.q ?? "");
  // The route's title leads until the person types; then the search of the text does.
  const [typed, setTyped] = useState(false);
  const useRoute = routed !== null && !typed;
  const match = useTitleMatch(text, !useRoute);

  // The best match once the title search has settled; a pick overrides it until the next search.
  const [best, setBest] = useState<TitleHit | null>(null);
  const [picked, setPicked] = useState<TitleHit | null>(null);
  const settledBest = match.short ? null : match.settled ? (match.hits[0] ?? null) : undefined;
  if (settledBest !== undefined && settledBest?.id !== best?.id) {
    setBest(settledBest);
    setPicked(null);
  }
  const hit = picked ?? best;
  const ref: TitleRef | null = useRoute
    ? routed
    : hit === null
      ? null
      : { kind: hit.kind === "show" ? "show" : "movie", tmdbId: hit.tmdbId };

  const details = useTitleDetails(ref ?? { kind: "movie", tmdbId: 0 }, ref !== null);
  const known = ref !== null && details.data?.tmdbId === ref.tmdbId ? details.data : null;
  const name = hit !== null && !useRoute ? titleName(hit) : known === null ? "" : titleName(known);

  // A title that came with the route fills the field once its name is known.
  const shownText = useRoute && text === "" && known !== null ? titleName(known) : text;

  // The confirmed title goes into the address, so a reload or a copied link opens it again.
  const param = ref === null ? undefined : titleParam(ref);
  useEffect(() => {
    if (param === undefined || param === search.title) return;
    void navigate({
      to: "/torrents",
      search: { title: param } as TorrentsSearch,
      replace: true,
    });
  }, [param, search.title, navigate]);

  const target = ref === null ? null : targetOf(ref, search, routed);
  const releases = useReleaseSearch(target);
  const matching = useMemo(
    () => releases.releases.filter((release) => release.matching),
    [releases.releases],
  );
  const [sort, setSort] = useRememberedSort();
  const [chosen, setChosen] = useState<Chosen>("all");
  const counts = useMemo(() => tierCounts(matching), [matching]);
  // Before the first release arrives the chips are quiet: no "0" counts, nothing to pick.
  const hasResults = matching.length > 0;
  const shown = useMemo(() => {
    const inTier = chosen === "all" ? matching : matching.filter((r) => tierOf(r) === chosen);
    return sortReleases(inTier, sort, (release) => sourceText(release, releases.sources));
  }, [matching, chosen, sort, releases.sources]);

  const owned: OwnedCopy | null = useMemo(() => {
    const copy = known?.libraryCopy;
    if (copy === undefined || copy === null) return null;
    const quality = [copy.resolution === "unknown" ? "" : copy.resolution, hdrWord(copy.hdr)]
      .filter(Boolean)
      .join(" ");
    return { quality, size: copy.size };
  }, [known]);

  const asked = releases.sources.filter((source) => source.skipped === null).length;
  const countText =
    ref === null || (matching.length === 0 && releases.searching)
      ? ""
      : t("torrentsPage.count", {
          results: t("torrentsPage.count.results", { count: matching.length }),
          sources: t("torrentsPage.count.sources", { count: asked }),
        });

  const clear = () => {
    setText("");
    setTyped(true);
    setBest(null);
    setPicked(null);
    void navigate({ to: "/torrents", search: {} as TorrentsSearch, replace: true });
  };

  const searching = ref === null ? !match.short && !match.settled : releases.searching;
  const inLibrary = known?.inLibrary ?? null;
  const copyTitle = known === null ? "" : titleName(known);
  const ownedLine =
    owned === null ? null : (
      <span className="flex min-w-0 items-center gap-2 text-(length:--fs-13) text-text-2">
        <Icon icon={Check} className="text-ok" />
        <span className="truncate">
          {t("torrentsPage.yours.head")}{" "}
          {inLibrary !== null && known?.kind === "movie" ? (
            <Link to="/movies/$movieId" params={{ movieId: inLibrary.itemId }} className="cv-link">
              {copyTitle}
            </Link>
          ) : (
            copyTitle
          )}{" "}
          · {owned.quality}
          {owned.size === null ? "" : ` · ${format.size(owned.size)}`}
        </span>
      </span>
    );

  let body: React.ReactNode;
  if (ref === null) {
    body = match.short ? (
      <Message>
        <p className="m-0">{t("torrentsPage.idle")}</p>
      </Message>
    ) : match.failed ? (
      <Message role="alert">
        <p className="m-0 text-text">{t("torrents.picker.failed")}</p>
        <Button onClick={match.retry}>{t("torrents.retryAll")}</Button>
      </Message>
    ) : match.settled ? (
      <Message role="status">
        <p className="m-0">{t("torrents.picker.nothing", { query: text.trim() })}</p>
      </Message>
    ) : (
      <Rows />
    );
  } else if (matching.length === 0) {
    body = releases.searching ? (
      <Rows />
    ) : noSourceAnswered(releases.providers, releases.searching) ? (
      <Message role="alert">
        <p className="m-0 text-text">{t("torrents.picker.failed")}</p>
        <Button onClick={releases.retryAll}>{t("torrents.retryAll")}</Button>
      </Message>
    ) : (
      <Message role="status">
        <p className="m-0">{t("torrentsPage.noReleases", { title: name })}</p>
        <Button onClick={releases.restart}>{t("torrents.empty.searchAgain")}</Button>
      </Message>
    );
  } else if (shown.length === 0) {
    body = (
      <Message role="status">
        <p className="m-0">{t("torrentsPage.noTier", { tier: chosen })}</p>
        <Button onClick={() => setChosen("all")}>{t("torrents.filters.clear")}</Button>
      </Message>
    );
  } else {
    body = (
      <ReleaseTable
        releases={shown}
        sources={releases.sources}
        owned={owned}
        sort={sort}
        onSort={setSort}
      />
    );
  }

  return (
    <section
      aria-label={t("torrentsPage.region")}
      data-testid="torrents-page"
      className="relative flex h-full min-h-0 flex-col"
    >
      <h1 className="sr-only">{t("torrentsPage.region")}</h1>
      <div className="relative flex shrink-0 flex-col gap-2.5 px-6 pt-4 pb-3">
        <span
          aria-hidden="true"
          className="absolute right-0 bottom-0 left-0 h-px bg-line-section"
        />
        <Field
          type="text"
          role="searchbox"
          autoComplete="off"
          spellCheck={false}
          ref={field}
          height="search"
          icon={Search}
          aria-label={t("torrentsPage.field")}
          placeholder={t("torrentsPage.placeholder")}
          value={shownText}
          boxClassName="[&_input]:text-(length:--fs-16)"
          onChange={(event) => {
            setText(event.target.value);
            setTyped(true);
          }}
          onKeyDown={(event) => {
            if (event.key === "Escape" && shownText !== "") {
              event.preventDefault();
              event.stopPropagation();
              clear();
            }
          }}
          trailing={
            <>
              <span
                aria-live="polite"
                className="shrink-0 text-(length:--fs-12) whitespace-nowrap text-text-muted"
              >
                {countText}
              </span>
              {shownText === "" ? null : (
                <IconButton
                  aria-label={t("torrentsPage.clear")}
                  icon={X}
                  size="sm"
                  onClick={clear}
                />
              )}
            </>
          }
        />
        <TitleLine
          name={name}
          kind={ref?.kind ?? "movie"}
          choices={useRoute ? [] : match.hits}
          currentId={hit?.id ?? ""}
          onPick={setPicked}
        />
        <div className="flex h-6 items-center gap-1.5">
          <div
            role="group"
            aria-label={t("torrentsPage.chips")}
            className="flex items-center gap-1.5"
          >
            <Chip
              active={chosen === "all"}
              disabled={!hasResults}
              {...(hasResults ? { count: format.number(matching.length) } : {})}
              onClick={() => setChosen("all")}
            >
              {t("torrentsPage.chip.all")}
            </Chip>
            {TIERS.map((tier) => (
              <Chip
                key={tier}
                active={chosen === tier}
                disabled={!hasResults}
                {...(hasResults ? { count: format.number(counts[tier]) } : {})}
                onClick={() => setChosen(tier)}
              >
                {tier}
              </Chip>
            ))}
          </div>
          <span className="grow" />
          {ownedLine}
        </div>
      </div>
      {online ? null : (
        <div
          role="status"
          className="mx-6 mt-3 flex shrink-0 items-center gap-3 rounded-(--r-10) border border-solid border-line-section bg-surface-card px-3 py-2"
        >
          <Icon icon={WifiOff} />
          <p className="m-0 flex-1 text-text">{t("torrents.offline")}</p>
          <Button onClick={releases.retryAll}>{t("torrents.retryAll")}</Button>
        </div>
      )}
      <div aria-busy={searching} className="flex min-h-0 flex-1 flex-col">
        {body}
      </div>
    </section>
  );
}

/**
 * `/torrents`, behind the responsibility notice: the first visit shows the notice and nothing
 * else, so no source is contacted until it is accepted (setting `torrents.notice-accepted`).
 */
export function TorrentsPage() {
  const accepted = useTorrentNotice();
  if (accepted === null) {
    return <section aria-label={t("torrentsPage.region")} data-testid="torrents-page" />;
  }
  return accepted ? <TorrentsContent /> : <Notice />;
}
