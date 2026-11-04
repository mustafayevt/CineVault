import { useNavigate, useSearch } from "@tanstack/react-router";
import { BookmarkCheck, BookmarkPlus, Check, ListFilter, Search, WifiOff, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Chip } from "../../components/Chip";
import { Field } from "../../components/Field";
import { IconButton } from "../../components/IconButton";
import { Icon } from "../../components/Icon";
import { Skeleton } from "../../components/Skeleton";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import type { QualityTier } from "../../components/QualityBadge";
import { SuggestPanel } from "../search/SuggestPanel";
import type { SuggestItem } from "../search/SuggestPanel";
import { useCombobox } from "../search/useCombobox";
import { DEFAULT_FEED_VIEW } from "./feedView";
import type { FeedView } from "./feedView";
import { LatestFeed } from "./LatestFeed";
import { holdsTitle, showHoldings, showMarkOf, verdictOf } from "./ownedCopy";
import type { ShowMark } from "./ownedCopy";
import { isEmptyFilter, matchesFilter, parseReleaseFilter } from "./releaseFilter";
import { ReleaseTable } from "./ReleaseTable";
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
import type { ReleaseSearchTarget, TitleHit, TorrentRelease } from "./torrentBackend";
import { noSourceAnswered, titleName } from "./torrentModel";
import {
  onlyParam,
  parseOnly,
  parseTitleRef,
  scopeOf,
  titleParam,
  wishlistAddLink,
} from "./torrentsSearch";
import type { LibraryFilter, TitleRef, TorrentsSearch } from "./torrentsSearch";
import { useOnline } from "./useOnline";
import { useReleaseSearch } from "./useReleaseSearch";
import { useTitleMatch } from "./useTitleMatch";
import { useTorrentNotice } from "./useTorrentNotice";
import { TorrentsViewSwitch } from "./TorrentsViewSwitch";
import { onWishlist } from "./wishlistModel";
import { useWishlist } from "./wishlistQueries";
import { YouHaveThis } from "./YouHaveThis";

type Chosen = "all" | QualityTier;

const NO_FILTERS: ReadonlySet<LibraryFilter> = new Set();

const MARK_VARIANT: Record<ShowMark, "ok" | "neutral"> = {
  gap: "ok",
  held: "neutral",
  better: "ok",
};

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
      className="flex h-full min-h-0 flex-col gap-4 px-6 pt-4 @max-[640px]/pane:px-4"
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
 * "Add to Wishlist" for the picked title, which opens the Wishlist with its editor (a show starts
 * from the season or episode chosen here), or "On your Wishlist" when it is there already.
 */
function WishlistButton({
  target,
  search,
  routed,
}: {
  target: TitleRef;
  search: TorrentsSearch;
  routed: TitleRef | null;
}) {
  const navigate = useNavigate();
  const list = useWishlist();
  const listed = list.data === undefined ? null : onWishlist(list.data, target.kind, target.tmdbId);
  if (listed !== null) {
    return (
      <Button
        size="sm"
        variant="ghost"
        icon={BookmarkCheck}
        onClick={() => void navigate({ to: "/torrents/wishlist" })}
      >
        {t("wishlist.onList")}
      </Button>
    );
  }
  const scoped = target.kind === "show" && routed?.tmdbId === target.tmdbId;
  return (
    <Button
      size="sm"
      variant="ghost"
      icon={BookmarkPlus}
      onClick={() =>
        void navigate({
          href: wishlistAddLink(
            target,
            scoped ? search.season : undefined,
            scoped ? search.episode : undefined,
          ),
        })
      }
    >
      {t("wishlist.add")}
    </Button>
  );
}

/**
 * `/torrents` (spec `torrent-search`): one search field. After a short pause in typing the field
 * suggests titles (movies and shows from the metadata provider, the ones you own marked), and no
 * release is searched until one is picked. The picked title shows as a chip above the results
 * with an × to change it, and its releases list as the sources answer, with resolution chips and
 * counts, a sortable table, and a hint against the copy the library already holds. Until a title
 * is picked the page lists the newest releases of the sources (`LatestFeed`). The route
 * carries a confirmed title (`?title=movie:123`, from a collection or a title page) or the typed
 * text (`?q=`).
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
  // The title the person picked from the suggestions. Nothing is searched before this.
  const [picked, setPicked] = useState<TitleHit | null>(null);
  // The suggestions are looked up while no title is chosen.
  const match = useTitleMatch(text, !useRoute && picked === null);

  const hit = picked;
  const ref: TitleRef | null = useRoute
    ? routed
    : hit === null
      ? null
      : { kind: hit.kind === "show" ? "show" : "movie", tmdbId: hit.tmdbId };

  // The suggestions keep their last rows while the next search is out, so the list never blinks.
  const suggesting = ref === null;
  const [kept, setKept] = useState<readonly TitleHit[]>([]);
  const fresh = suggesting ? match.hits : [];
  if (match.short && kept.length > 0) setKept([]);
  else if (fresh.length > 0 && fresh !== kept) setKept(fresh);
  const suggested: readonly TitleHit[] =
    !suggesting || match.short ? [] : fresh.length > 0 || match.settled ? fresh : kept;
  const [anchor, setAnchor] = useState<HTMLDivElement | null>(null);
  const pick = (choice: TitleHit) => {
    setPicked(choice);
    setText(titleName(choice));
    setTyped(true);
  };
  // The "Latest releases" feed shows while no title is chosen; its view outlives a search.
  const [feedView, setFeedView] = useState<FeedView>(DEFAULT_FEED_VIEW);
  const [feedCount, setFeedCount] = useState<number | null>(null);
  const combobox = useCombobox({
    anchor,
    ids: suggested.map((choice) => choice.id),
    showing: suggesting && !match.short && (suggested.length > 0 || match.settled),
    onSelect: (id) => {
      const choice = suggested.find((entry) => entry.id === id);
      if (choice !== undefined) pick(choice);
    },
  });
  const suggestions: SuggestItem[] = suggested.map((choice) => ({
    id: choice.id,
    label: titleName(choice),
    poster:
      choice.posterKey === null ? null : { key: choice.posterKey, thumbhash: null, ambient: null },
    meta: t(choice.kind === "show" ? "torrents.title.show" : "torrents.title.movie"),
    badge:
      choice.inLibrary === null ? undefined : (
        <span className="flex shrink-0 items-center gap-1 text-(length:--fs-12) text-ok">
          <Icon icon={Check} size={14} />
          {t("torrentsPage.suggest.owned")}
        </span>
      ),
  }));

  const details = useTitleDetails(ref ?? { kind: "movie", tmdbId: 0 }, ref !== null);
  const known = ref !== null && details.data?.tmdbId === ref.tmdbId ? details.data : null;
  const name = hit !== null && !useRoute ? titleName(hit) : known === null ? "" : titleName(known);

  // A title that came with the route fills the field once its name is known.
  const shownText = useRoute && text === "" && known !== null ? titleName(known) : text;

  // The confirmed title goes into the address, so a reload or a copied link opens it again.
  const param = ref === null ? undefined : titleParam(ref);
  // The library chips belong to one title: another title starts with them off.
  const [library, setLibrary] = useState<{
    title: string | undefined;
    on: ReadonlySet<LibraryFilter>;
  }>(() => ({ title: search.title, on: parseOnly(search.only) }));
  const filters = library.title === param ? library.on : NO_FILTERS;
  const setFilters = (on: ReadonlySet<LibraryFilter>) => setLibrary({ title: param, on });
  const toggleFilter = (filter: LibraryFilter) => {
    const next = new Set(filters);
    if (next.has(filter)) next.delete(filter);
    else next.add(filter);
    setFilters(next);
  };
  const only = onlyParam(filters);
  useEffect(() => {
    if (param === undefined) {
      // The title was changed by typing: the address stops naming it.
      if (search.title !== undefined && typed) {
        void navigate({ to: "/torrents", search: {} as TorrentsSearch, replace: true });
      }
      return;
    }
    if (param === search.title && only === search.only) return;
    void navigate({
      to: "/torrents",
      search: {
        title: param,
        ...(param === search.title && search.season !== undefined ? { season: search.season } : {}),
        ...(param === search.title && search.episode !== undefined
          ? { episode: search.episode }
          : {}),
        ...(only === undefined ? {} : { only }),
      } as TorrentsSearch,
      replace: true,
    });
  }, [param, only, search.title, search.only, search.season, search.episode, typed, navigate]);

  const target = ref === null ? null : targetOf(ref, search, routed);
  const releases = useReleaseSearch(target);
  const matching = useMemo(
    () => releases.releases.filter((release) => release.matching),
    [releases.releases],
  );
  // The "Filter results" text belongs to one title, like the library chips.
  const [typedFilter, setTypedFilter] = useState<{ title: string | undefined; text: string }>(
    () => ({ title: search.title, text: "" }),
  );
  const filterText = typedFilter.title === param ? typedFilter.text : "";
  const setFilterText = (text: string) => setTypedFilter({ title: param, text });
  const textFilter = useMemo(() => parseReleaseFilter(filterText), [filterText]);
  // The text narrows first, so the chip counts say what each chip would show.
  const narrowed = useMemo(
    () =>
      isEmptyFilter(textFilter)
        ? matching
        : matching.filter((release) => matchesFilter(release.name, textFilter)),
    [matching, textFilter],
  );
  const [sort, setSort] = useRememberedSort();
  const [chosen, setChosen] = useState<Chosen>("all");
  const counts = useMemo(() => tierCounts(narrowed), [narrowed]);
  // Before the first release arrives the chips are quiet: no "0" counts, nothing to pick.
  const hasResults = matching.length > 0;
  // What the library holds: the copy a movie's releases are compared with, a show's episodes.
  const owned = known?.kind === "movie" ? (known.libraryCopies[0] ?? null) : null;
  const holdings = useMemo(
    () =>
      known?.kind === "show" && known.heldEpisodes.length > 0
        ? showHoldings(known.heldEpisodes, known.missingEpisodes)
        : null,
    [known],
  );
  const marks = useMemo(() => {
    const out = new Map<string, ShowMark>();
    if (holdings === null) return out;
    for (const release of matching) {
      const mark = showMarkOf(release, holdings);
      if (mark !== null) out.set(release.id, mark);
    }
    return out;
  }, [matching, holdings]);
  const betterCount = narrowed.filter((release) => verdictOf(release) === "better").length;
  const gapCount = narrowed.filter((release) => marks.get(release.id) === "gap").length;
  const offerBetter =
    owned !== null || (holdings !== null && matching.some((r) => verdictOf(r) !== null));
  const offerGaps = holdings !== null;
  const betterOn = filters.has("better") && offerBetter;
  const gapsOn = filters.has("gaps") && offerGaps;
  const shown = useMemo(() => {
    const kept = narrowed.filter(
      (release) =>
        (chosen === "all" || tierOf(release) === chosen) &&
        (!betterOn || verdictOf(release) === "better") &&
        (!gapsOn || marks.get(release.id) === "gap"),
    );
    return sortReleases(kept, sort, (release) => sourceText(release, releases.sources));
  }, [narrowed, chosen, betterOn, gapsOn, marks, sort, releases.sources]);
  const nameExtra = useMemo(
    () =>
      holdings === null
        ? undefined
        : (release: TorrentRelease) => {
            const mark = marks.get(release.id);
            if (mark === undefined) return null;
            return (
              <span
                className="ml-2 shrink-0"
                title={t(`torrentsPage.mark.${mark}.tip`)}
                data-testid="show-mark"
              >
                <Tag variant={MARK_VARIANT[mark]}>{t(`torrentsPage.mark.${mark}`)}</Tag>
              </span>
            );
          },
    [holdings, marks],
  );
  // "Search anyway" folds the card for the title it was pressed on.
  const [foldedFor, setFoldedFor] = useState<string | null>(null);

  const asked = releases.sources.filter((source) => source.skipped === null).length;
  const countText =
    ref === null
      ? feedCount === null || shownText !== ""
        ? ""
        : t("torrentsFeed.count", { count: feedCount })
      : matching.length === 0 && releases.searching
        ? ""
        : t("torrentsPage.count", {
            results: t("torrentsPage.count.results", { count: matching.length }),
            sources: t("torrentsPage.count.sources", { count: asked }),
          });

  const clear = () => {
    setText("");
    setTyped(true);
    setPicked(null);
    void navigate({ to: "/torrents", search: {} as TorrentsSearch, replace: true });
  };

  const searching = ref === null ? false : releases.searching;
  const card =
    known === null || !holdsTitle(known) ? null : (
      <YouHaveThis
        details={known}
        folded={foldedFor === param}
        onFold={(fold) => setFoldedFor(fold ? (param ?? null) : null)}
        onLook={() => {
          const next = new Set(filters);
          next.add(known.kind === "show" ? "gaps" : "better");
          setFilters(next);
        }}
      />
    );

  let body: React.ReactNode;
  if (ref === null) {
    // No release is searched until a title is picked: the field's suggestions do the finding.
    // Meanwhile the newest releases of the sources are listed.
    body = (
      <LatestFeed
        view={feedView}
        onView={setFeedView}
        onSearch={(hit) => {
          pick(hit);
          field.current?.focus();
        }}
        onCount={setFeedCount}
      />
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
        <p className="m-0">
          {narrowed.length === 0
            ? t("torrentsPage.noText", { text: filterText.trim() })
            : betterOn || gapsOn
              ? t("torrentsPage.noLibraryFilter")
              : t("torrentsPage.noTier", { tier: chosen })}
        </p>
        <Button
          onClick={() => {
            setChosen("all");
            setFilters(NO_FILTERS);
            setFilterText("");
          }}
        >
          {t("torrents.filters.clear")}
        </Button>
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
        {...(nameExtra === undefined ? {} : { nameExtra })}
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
      <div className="relative flex shrink-0 flex-col gap-2.5 px-6 pt-4 pb-3 @max-[640px]/pane:px-4">
        <span
          aria-hidden="true"
          className="absolute right-0 bottom-0 left-0 h-px bg-line-section"
        />
        <div className="flex min-w-0 items-center gap-3 @max-[640px]/pane:flex-col @max-[640px]/pane:items-stretch">
          <TorrentsViewSwitch value="search" />
          <div ref={setAnchor} className="min-w-0 flex-1">
            <Field
              type="text"
              autoComplete="off"
              spellCheck={false}
              ref={field}
              height="search"
              icon={Search}
              aria-label={t("torrentsPage.field")}
              placeholder={t("torrentsPage.placeholder")}
              value={shownText}
              boxClassName="[&_input]:text-(length:--fs-16)"
              {...combobox.fieldProps}
              onChange={(event) => {
                setText(event.target.value);
                setTyped(true);
                // A new text is a new title: the old one and its releases go.
                setPicked(null);
                combobox.typed();
              }}
              onKeyDown={(event) => {
                combobox.fieldProps.onKeyDown(event);
                if (event.defaultPrevented) return;
                if (event.key === "Enter" && suggesting && suggested[0] !== undefined) {
                  // Enter with nothing highlighted takes the best suggestion.
                  event.preventDefault();
                  combobox.select(suggested[0].id);
                } else if (event.key === "Escape" && shownText !== "") {
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
            <SuggestPanel
              combobox={combobox}
              label={t("torrentsPage.suggest.label")}
              groups={[{ id: "titles", label: "", items: suggestions }]}
              term={text.trim()}
              maxWidth={720}
              {...(match.settled && suggested.length === 0
                ? {
                    empty: match.failed ? (
                      <span className="flex items-center gap-3">
                        <span className="text-text">{t("torrents.picker.failed")}</span>
                        <Button size="sm" onClick={match.retry}>
                          {t("torrents.retryAll")}
                        </Button>
                      </span>
                    ) : (
                      t("torrents.picker.nothing", { query: text.trim() })
                    ),
                  }
                : {})}
            />
          </div>
        </div>
        <TitleLine
          name={name}
          kind={ref?.kind ?? "movie"}
          onChange={() => {
            clear();
            field.current?.focus();
          }}
          {...(ref === null
            ? {}
            : { extra: <WishlistButton target={ref} search={search} routed={routed} /> })}
        />
        {card}
        <div
          className={
            ref === null
              ? "hidden"
              : "flex h-(--h-field) items-center gap-1.5 @max-[640px]/pane:h-auto @max-[640px]/pane:flex-wrap @max-[640px]/pane:gap-y-2"
          }
        >
          <div
            role="group"
            aria-label={t("torrentsPage.chips")}
            className="flex items-center gap-1.5"
          >
            <Chip
              active={chosen === "all"}
              disabled={!hasResults}
              {...(hasResults ? { count: format.number(narrowed.length) } : {})}
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
          {offerBetter || offerGaps ? (
            <div
              role="group"
              aria-label={t("torrentsPage.chip.library")}
              className="flex items-center gap-1.5 border-l border-solid border-line-section pl-1.5 @max-[640px]/pane:border-l-0 @max-[640px]/pane:pl-0"
            >
              {offerBetter ? (
                <Chip
                  active={betterOn}
                  disabled={!hasResults}
                  {...(hasResults ? { count: format.number(betterCount) } : {})}
                  onClick={() => toggleFilter("better")}
                >
                  {t("torrentsPage.chip.better")}
                </Chip>
              ) : null}
              {offerGaps ? (
                <Chip
                  active={gapsOn}
                  disabled={!hasResults}
                  {...(hasResults ? { count: format.number(gapCount) } : {})}
                  onClick={() => toggleFilter("gaps")}
                >
                  {t("torrentsPage.chip.gaps")}
                </Chip>
              ) : null}
            </div>
          ) : null}
          <Field
            type="text"
            autoComplete="off"
            spellCheck={false}
            icon={ListFilter}
            aria-label={t("torrentsPage.filter")}
            placeholder={t("torrentsPage.filter")}
            title={t("torrentsPage.filter.tip")}
            disabled={!hasResults}
            value={filterText}
            className="ml-auto w-60 shrink-0 @max-[640px]/pane:ml-0 @max-[640px]/pane:w-full"
            onChange={(event) => setFilterText(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Escape" && filterText !== "") {
                event.preventDefault();
                event.stopPropagation();
                setFilterText("");
              }
            }}
            trailing={
              filterText === "" ? null : (
                <IconButton
                  aria-label={t("torrentsPage.filter.clear")}
                  icon={X}
                  size="sm"
                  onClick={() => setFilterText("")}
                />
              )
            }
          />
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
      <p
        data-testid="torrent-responsibility"
        className="m-0 shrink-0 border-t border-solid border-line-section px-6 py-1.5 text-(length:--fs-12) text-text-muted @max-[640px]/pane:px-4"
      >
        {t("torrentsPage.responsibility")}
      </p>
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
