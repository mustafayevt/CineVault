import { Loader2, RefreshCw } from "lucide-react";
import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Chip } from "../../components/Chip";
import { IconButton } from "../../components/IconButton";
import { Icon } from "../../components/Icon";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Select } from "../../components/Select";
import { Skeleton } from "../../components/Skeleton";
import type { TableSort } from "../../components/Table";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { FeedPanel } from "./FeedPanel";
import { ReleaseTable } from "./ReleaseTable";
import { TIERS, sortReleases, tierCounts, tierOf, useFeedSort } from "./releaseSort";
import type { FeedRow, TitleHit, TorrentRelease } from "./torrentBackend";
import { siteName } from "./torrentModel";
import { isLowQuality } from "./feedView";
import type { FeedView } from "./feedView";
import { useLatestFeed } from "./useLatestFeed";
import type { FeedSourceView } from "./useLatestFeed";

/** The sort choices of the drop-down, as table sorts. */
const SORTS: readonly { value: "newest" | "seeders" | "size" | "quality"; sort: TableSort }[] = [
  { value: "newest", sort: { column: "age", direction: "asc" } },
  { value: "seeders", sort: { column: "seeders", direction: "desc" } },
  { value: "size", sort: { column: "size", direction: "desc" } },
  { value: "quality", sort: { column: "quality", direction: "desc" } },
];

/** Below this width of the list, an open panel also hides Size and Seeders. */
const WIDE_ENOUGH = 1100;
const BESIDE_PANEL = new Set(["source", "age"]);
const BESIDE_PANEL_NARROW = new Set(["source", "age", "size", "seeders"]);

/** How close to the end of the list the next page is read. */
const NEAR_END = 8;

/** The names of the sources in a state, as one line: "YTS, The Pirate Bay". */
function names(sources: readonly FeedSourceView[], keep: (source: FeedSourceView) => boolean) {
  return sources
    .filter(keep)
    .map((source) => siteName(source.name))
    .join(", ");
}

interface LatestFeedProps {
  view: FeedView;
  onView: (view: FeedView) => void;
  /** Searches every source for a title the panel matched. */
  onSearch: (hit: TitleHit) => void;
  /** Tells the page how many releases are listed, for the count in the field. */
  onCount?: (count: number | null) => void;
}

/**
 * The "Latest releases" feed of the Torrents page (spec `torrent-search` "Latest releases feed"):
 * the newest releases of the sources in the release table, streaming in as each source answers,
 * with All, Movies and TV, a sort, the resolution chips and "Hide CAM and TS". Scrolling near the
 * end reads the next page of every source that has one. The Details button, Enter or a
 * double-click opens the side panel, which looks the title up only then.
 */
export function LatestFeed({ view, onView, onSearch, onCount }: LatestFeedProps) {
  const format = useFormat();
  const feed = useLatestFeed(view.kind, true);
  const [sort, setSort] = useFeedSort();
  const [openId, setOpenId] = useState<string | null>(null);
  const [picks, setPicks] = useState<ReadonlyMap<string, TitleHit>>(new Map());

  const byId = useMemo(() => new Map(feed.rows.map((row) => [row.release.id, row])), [feed.rows]);
  const visible = useMemo(
    () => (view.hideLow ? feed.rows.filter((row) => !isLowQuality(row.release)) : feed.rows),
    [feed.rows, view.hideLow],
  );
  const releases = useMemo(
    // The table's own "Better" tag compares with one owned copy; the feed marks rows itself.
    () => visible.map((row): TorrentRelease => ({ ...row.release, upgrade: null })),
    [visible],
  );
  const counts = useMemo(() => tierCounts(releases), [releases]);
  const shown = useMemo(() => {
    const inTier =
      view.tier === "all" ? releases : releases.filter((release) => tierOf(release) === view.tier);
    return sortReleases(inTier, sort, (release) => {
      const [first] = release.sources;
      return first === undefined ? "" : (feed.sources.find((s) => s.id === first)?.name ?? first);
    });
  }, [releases, view.tier, sort, feed.sources]);

  useEffect(() => {
    onCount?.(feed.firstLoad ? null : visible.length);
  }, [onCount, feed.firstLoad, visible.length]);

  const { hasMore, loading, paused, loadMore } = feed;
  const onVisibleRange = useCallback(
    ({ end }: { start: number; end: number }) => {
      if (hasMore && !loading && !paused && end >= shown.length - NEAR_END) loadMore();
    },
    [hasMore, loading, paused, loadMore, shown.length],
  );

  const open = openId === null ? undefined : byId.get(openId);
  const panelOpen = open !== undefined;
  // The table makes room for the panel by leaving out the columns the panel repeats.
  const area = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  useLayoutEffect(() => {
    const element = area.current;
    if (element === null) return;
    const measure = () => setWidth(element.clientWidth);
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);
  const hiddenColumns = panelOpen
    ? width > 0 && width < WIDE_ENOUGH
      ? BESIDE_PANEL_NARROW
      : BESIDE_PANEL
    : undefined;
  // Esc closes the details panel first (a menu that is open takes the key before it gets here).
  useEffect(() => {
    if (!panelOpen) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || event.defaultPrevented) return;
      event.preventDefault();
      event.stopPropagation();
      setOpenId(null);
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [panelOpen]);
  const siblings = useMemo(() => {
    if (open === undefined) return [];
    return feed.rows
      .filter((row) => row.titleKey === open.titleKey)
      .sort((a, b) => (b.release.published ?? 0) - (a.release.published ?? 0));
  }, [open, feed.rows]);

  const nameExtra = useCallback(
    (release: TorrentRelease) => {
      const row = byId.get(release.id);
      if (row === undefined || row.inLibrary === null) return null;
      const better = row.release.upgrade === "better";
      return (
        <span className="ml-2 shrink-0" title={row.inLibrary.name}>
          <Tag variant={better ? "ok" : "neutral"}>
            {t(better ? "torrentsFeed.betterCopy" : "torrentsFeed.inLibrary")}
          </Tag>
        </span>
      );
    },
    [byId],
  );
  const onDetails = useCallback((release: TorrentRelease) => setOpenId(release.id), []);
  const onSelectOne = useCallback(
    (release: TorrentRelease) => setOpenId((current) => (current === null ? null : release.id)),
    [],
  );

  const sortValue =
    SORTS.find(
      (choice) => choice.sort.column === sort.column && choice.sort.direction === sort.direction,
    )?.value ?? "custom";
  const sortOptions = [
    ...SORTS.map((choice) => ({
      value: choice.value,
      label: t(`torrentsFeed.sort.${choice.value}`),
    })),
    ...(sortValue === "custom" ? [{ value: "custom", label: t("torrentsFeed.sort.custom") }] : []),
  ];

  const noMore = names(feed.sources, (source) => source.state === "done");
  const unsupported = names(
    feed.sources,
    (source) => source.state === "unsupported" && source.kind !== "built-in",
  );
  const failed = names(feed.sources, (source) => source.state === "failed");
  const takingPart = feed.sources.filter(
    (source) =>
      source.state !== "unsupported" &&
      source.state !== "movies-only" &&
      source.state !== "tv-only",
  );

  let body: React.ReactNode;
  if (feed.firstLoad) {
    body = (
      <div aria-busy="true" className="flex flex-col px-6 pt-1">
        <span className="sr-only" role="status">
          {t("torrentsFeed.loading")}
        </span>
        {[0, 1, 2, 3, 4, 5, 6, 7].map((n) => (
          <Skeleton key={n} shape="row" />
        ))}
      </div>
    );
  } else if (feed.rows.length === 0) {
    body = (
      <div
        role="status"
        className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-text-2"
      >
        <p className="m-0">
          {t(
            feed.sources.length > 0 && takingPart.length === 0
              ? "torrentsFeed.noSources"
              : "torrentsFeed.nothing",
          )}
        </p>
        <Button icon={RefreshCw} onClick={feed.refresh}>
          {t("torrentsFeed.refresh")}
        </Button>
      </div>
    );
  } else if (shown.length === 0) {
    body = (
      <div
        role="status"
        className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center text-text-2"
      >
        <p className="m-0">{t("torrentsFeed.allHidden")}</p>
        <Button onClick={() => onView({ ...view, tier: "all", hideLow: false })}>
          {t("torrents.filters.clear")}
        </Button>
      </div>
    );
  } else {
    body = (
      <ReleaseTable
        aria-label={t("torrentsFeed.table")}
        storageKey="torrent-latest"
        releases={shown}
        sources={feed.sources}
        owned={null}
        sort={sort}
        onSort={setSort}
        qualitySortable
        onDetails={onDetails}
        onSelectOne={onSelectOne}
        nameExtra={nameExtra}
        onVisibleRange={onVisibleRange}
        {...(hiddenColumns === undefined ? {} : { hiddenColumns })}
      />
    );
  }

  return (
    <div data-testid="latest-feed" className="flex min-h-0 flex-1 flex-col">
      <div className="flex shrink-0 flex-wrap items-center gap-x-3 gap-y-2 px-6 pt-3 pb-2">
        <SegmentedControl
          aria-label={t("torrentsFeed.kind")}
          value={view.kind}
          onChange={(kind) => {
            setOpenId(null);
            onView({ ...view, kind });
          }}
          options={[
            { value: "all", label: t("torrentsFeed.kind.all") },
            { value: "movies", label: t("torrentsFeed.kind.movies") },
            { value: "tv", label: t("torrentsFeed.kind.tv") },
          ]}
        />
        <div
          role="group"
          aria-label={t("torrentsPage.chips")}
          className="flex items-center gap-1.5"
        >
          <Chip
            active={view.tier === "all"}
            {...(releases.length > 0 ? { count: format.number(releases.length) } : {})}
            onClick={() => onView({ ...view, tier: "all" })}
          >
            {t("torrentsPage.chip.all")}
          </Chip>
          {TIERS.map((tier) => (
            <Chip
              key={tier}
              active={view.tier === tier}
              {...(releases.length > 0 ? { count: format.number(counts[tier]) } : {})}
              onClick={() => onView({ ...view, tier })}
            >
              {tier}
            </Chip>
          ))}
        </div>
        <Chip
          active={view.hideLow}
          title={t("torrentsFeed.hideLow.tip")}
          onClick={() => onView({ ...view, hideLow: !view.hideLow })}
        >
          {t("torrentsFeed.hideLow")}
        </Chip>
        <span className="grow" />
        <span className="flex shrink-0 items-center gap-2">
          <Select
            aria-label={t("torrentsFeed.sort")}
            className="w-36"
            options={sortOptions}
            value={sortValue}
            onValueChange={(value) => {
              const choice = SORTS.find((entry) => entry.value === value);
              if (choice !== undefined) setSort(choice.sort);
            }}
          />
          <IconButton
            icon={RefreshCw}
            aria-label={t("torrentsFeed.refresh")}
            disabled={feed.loading && feed.rows.length === 0}
            onClick={() => {
              setOpenId(null);
              feed.refresh();
            }}
          />
        </span>
      </div>
      <div ref={area} className="flex min-h-0 flex-1">
        <div aria-busy={feed.loading} className="flex min-h-0 min-w-0 flex-1 flex-col">
          {body}
          {feed.firstLoad || feed.rows.length === 0 ? null : (
            <div
              data-testid="feed-status"
              className="flex shrink-0 flex-wrap items-center gap-x-4 gap-y-1 border-t border-solid border-line-section px-6 py-2 text-(length:--fs-12) text-text-muted"
            >
              {feed.loading ? (
                <span role="status" className="flex items-center gap-1.5">
                  <Icon icon={Loader2} size={14} className="animate-spin" />
                  {t("torrentsFeed.loadingMore")}
                </span>
              ) : null}
              {failed === "" ? null : (
                <span className="flex items-center gap-2">
                  <span className="text-text-2">
                    {t("torrentsFeed.source.failed", { names: failed })}
                  </span>
                  {feed.loading ? null : (
                    <Button size="sm" onClick={feed.loadMore}>
                      {t("torrentsFeed.retry")}
                    </Button>
                  )}
                </span>
              )}
              {noMore === "" ? null : (
                <span>{t("torrentsFeed.source.noMore", { names: noMore })}</span>
              )}
              {unsupported === "" ? null : (
                <span>{t("torrentsFeed.source.unsupported", { names: unsupported })}</span>
              )}
              <span className="grow" />
              {feed.hasMore && !feed.loading ? (
                <Button size="sm" onClick={feed.loadMore}>
                  {t("torrentsFeed.loadMore")}
                </Button>
              ) : null}
              {!feed.hasMore && !feed.loading ? <span>{t("torrentsFeed.end")}</span> : null}
            </div>
          )}
        </div>
        {open === undefined ? null : (
          <FeedPanel
            key={open.titleKey}
            row={open}
            siblings={siblings}
            sources={feed.sources}
            picked={picks.get(open.titleKey) ?? null}
            onPick={(hit) => setPicks((current) => new Map(current).set(open.titleKey, hit))}
            onSearch={onSearch}
            onShowRelease={(row: FeedRow) => setOpenId(row.release.id)}
            onClose={() => setOpenId(null)}
          />
        )}
      </div>
    </div>
  );
}
