import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { AlertTriangle, Check, ExternalLink, Search, Send, X } from "lucide-react";
import { useState } from "react";
import type { CSSProperties } from "react";

import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { FileSize } from "../../components/FileSize";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { QualityBadge } from "../../components/QualityBadge";
import { SectionLabel } from "../../components/SectionLabel";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { openRatingPage } from "../ratings/ratingPage";
import { titleLinks } from "./feedView";
import { tierOf } from "./releaseSort";
import { useTitleDetails } from "./titleQueries";
import { openRelease } from "./torrentActions";
import { TorrentRequestError, currentTorrentBackend } from "./torrentBackend";
import type { FeedLookup, FeedRow, TitleHit } from "./torrentBackend";
import { ageText, siteName, titleName } from "./torrentModel";
import { useTitleMatch } from "./useTitleMatch";

/** The lookup's query key: one per feed title. */
function lookupKey(row: FeedRow): readonly unknown[] {
  return ["torrents", "feedLookup", row.kind, row.imdbId, row.titleKey];
}

function kindOf(row: FeedRow): "movie" | "show" {
  return row.kind === "show" ? "show" : "movie";
}

/** What the release name says: "Movie · 2025" or "TV show · S02E05". */
function parsedLine(row: FeedRow): string {
  const parts = [t(row.kind === "show" ? "torrentsFeed.panel.show" : "torrentsFeed.panel.movie")];
  if (row.year !== null) parts.push(String(row.year));
  if (row.season !== null && row.episode !== null) {
    parts.push(
      t("torrentsFeed.panel.episode", {
        season: String(row.season).padStart(2, "0"),
        episode: String(row.episode).padStart(2, "0"),
      }),
    );
  } else if (row.season !== null) {
    parts.push(t("torrentsFeed.panel.season", { season: row.season }));
  }
  return parts.join(" · ");
}

/** "Pick another": the title search of the page, inside the panel. */
function PickAnother({ row, onPick }: { row: FeedRow; onPick: (hit: TitleHit) => void }) {
  const [text, setText] = useState(row.year === null ? row.title : `${row.title} ${row.year}`);
  const match = useTitleMatch(text, true);
  const kind = kindOf(row);
  const hits = match.hits.filter((hit) => hit.kind === kind).slice(0, 6);
  return (
    <div className="flex flex-col gap-2 px-5 pt-3">
      <Field
        type="text"
        autoComplete="off"
        spellCheck={false}
        icon={Search}
        aria-label={t("torrentsFeed.panel.pickLabel")}
        placeholder={t("torrentsFeed.panel.pickPlaceholder")}
        value={text}
        onChange={(event) => setText(event.target.value)}
      />
      <ul
        aria-label={t("torrentsFeed.panel.pickLabel")}
        className="m-0 flex list-none flex-col p-0"
      >
        {hits.map((hit) => (
          <li key={hit.id}>
            <button
              type="button"
              className="cv-hoverable flex w-full items-center gap-2 rounded-(--r-6) px-2 py-1.5 text-left"
              onClick={() => onPick(hit)}
            >
              <span className="min-w-0 flex-1 truncate">{titleName(hit)}</span>
              {hit.inLibrary === null ? null : <Icon icon={Check} size={14} className="text-ok" />}
            </button>
          </li>
        ))}
      </ul>
      {match.settled && hits.length === 0 ? (
        <p className="m-0 text-(length:--fs-12) text-text-muted">
          {t("torrentsFeed.panel.pickNothing", { query: text.trim() })}
        </p>
      ) : null}
    </div>
  );
}

interface FeedPanelProps {
  /** The release the panel is about. */
  row: FeedRow;
  /** The other releases of the same title in the feed, newest first. */
  siblings: readonly FeedRow[];
  /** The feed's sources, to name the release's. */
  sources: readonly { id: string; name: string }[];
  /** The title the person picked for this release's title, if any. */
  picked: TitleHit | null;
  onPick: (hit: TitleHit) => void;
  /** Searches every source for the title ("Search all releases"). */
  onSearch: (hit: TitleHit) => void;
  onShowRelease: (row: FeedRow) => void;
  onClose: () => void;
  /** True when the panel has just opened: it slides in. Switching titles inside it does not. */
  entering?: boolean;
}

/**
 * The side panel of a feed release: what the release name says at once, then (one lookup per
 * title, kept) the TMDB title it is most likely to be with its poster, plot, rating and links,
 * the library link, the other releases of the title in the list, and Send to torrent app. A weak
 * match says so and offers "Pick another"; without a TMDB key the panel shows the name's reading.
 */
export function FeedPanel({
  row,
  siblings,
  sources,
  picked,
  onPick,
  onSearch,
  onShowRelease,
  onClose,
  entering = false,
}: FeedPanelProps) {
  const format = useFormat();
  const [picking, setPicking] = useState(false);
  const lookup = useQuery<FeedLookup, Error>({
    queryKey: lookupKey(row),
    queryFn: () =>
      currentTorrentBackend().feedLookup({
        kind: row.kind,
        title: row.title,
        year: row.year,
        imdbId: row.imdbId,
      }),
    enabled: picked === null,
    staleTime: Infinity,
    gcTime: Infinity,
    retry: false,
  });
  const hit = picked ?? lookup.data?.hit ?? null;
  const weak = picked === null && lookup.data !== undefined && lookup.data.weak;
  const details = useTitleDetails({ kind: kindOf(row), tmdbId: hit?.tmdbId ?? 0 }, hit !== null);
  const known = hit !== null && details.data?.tmdbId === hit.tmdbId ? details.data : null;
  const noKey =
    picked === null &&
    lookup.error instanceof TorrentRequestError &&
    (lookup.error.code === "needs_key" || lookup.error.code === "key_rejected");
  const failed = picked === null && lookup.error !== null && !noKey;
  const looking = picked === null && lookup.isPending;

  const name = hit === null ? row.title : hit.title;
  const year = hit?.year ?? row.year;
  const posterKey = known?.posterKey ?? hit?.posterKey ?? null;
  const overview = known?.overview ?? hit?.overview ?? null;
  const imdbId = known?.imdbId ?? hit?.imdbId ?? row.imdbId;
  const inLibrary = known?.inLibrary ?? hit?.inLibrary ?? row.inLibrary;
  const links = hit === null ? [] : titleLinks(kindOf(row), hit.tmdbId, imdbId);
  const others = siblings.filter((other) => other.release.id !== row.release.id);
  const sourceNames = row.release.sources
    .map((id) => siteName(sources.find((source) => source.id === id)?.name ?? id))
    .join(", ");
  const age = ageText(row.release.published);

  return (
    <aside
      aria-label={t("torrentsFeed.panel.label")}
      data-testid="feed-panel"
      style={
        entering ? ({ "--cv-slide-x": "24px", "--cv-slide-y": "0px" } as CSSProperties) : undefined
      }
      className={cx(
        "flex w-[380px] shrink-0 flex-col overflow-y-auto border-l border-solid border-line-section bg-surface-card",
        entering && "cv-slide-up",
      )}
    >
      <div className="flex items-start gap-3.5 px-5 pt-4">
        <div className="relative aspect-[2/3] w-[84px] shrink-0 overflow-hidden rounded-(--r-10) bg-surface-active">
          {posterKey === null ? (
            <PlaceholderPoster title={name} className="size-full" />
          ) : (
            <CachedImage imageKey={posterKey} size="list" alt="" className="size-full" />
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-1 pt-0.5">
          <h2 className="m-0 text-(length:--fs-16) leading-[1.2] font-(weight:--fw-700)">
            {year === null ? name : `${name} (${year})`}
          </h2>
          <p className="m-0 text-(length:--fs-12) text-text-muted">{parsedLine(row)}</p>
          {known === null ? null : (
            <p className="m-0 text-(length:--fs-12) text-text-2">
              {[
                known.certification,
                known.genres.slice(0, 3).join(", "),
                known.rating === null
                  ? null
                  : t("torrentsFeed.panel.rating", { rating: format.number(known.rating) }),
              ]
                .filter((part) => part !== null && part !== "")
                .join(" · ")}
            </p>
          )}
          {picked === null ? null : (
            <p className="m-0 text-(length:--fs-12) text-text-muted">
              {t("torrentsFeed.panel.picked")}
            </p>
          )}
        </div>
        <IconButton
          icon={X}
          aria-label={t("torrentsFeed.panel.close")}
          size="sm"
          onClick={onClose}
        />
      </div>

      {looking ? (
        <div role="status" aria-busy="true" className="flex flex-col gap-2 px-5 pt-3.5">
          <span className="sr-only">{t("torrentsFeed.panel.looking")}</span>
          <Skeleton shape="line" className="w-full" />
          <Skeleton shape="line" className="w-full" />
        </div>
      ) : null}

      {noKey ? (
        <p role="status" className="m-0 px-5 pt-3.5 text-(length:--fs-13) text-text-2">
          {t("torrentsFeed.panel.noKey")}
        </p>
      ) : null}
      {failed ? (
        <p role="alert" className="m-0 px-5 pt-3.5 text-(length:--fs-13) text-danger">
          {t("torrentsFeed.panel.failed", { message: lookup.error.message })}
        </p>
      ) : null}
      {!looking && !noKey && !failed && (weak || hit === null) && picked === null ? (
        <div
          role="status"
          className="mx-5 mt-3.5 flex items-start gap-2 rounded-(--r-6) border border-solid border-line-section px-3 py-2 text-(length:--fs-13)"
        >
          <Icon icon={AlertTriangle} size={16} className="mt-0.5 shrink-0 text-warn" />
          <span className="flex-1">
            {t(hit === null ? "torrentsFeed.panel.notFound" : "torrentsFeed.panel.weak")}
          </span>
        </div>
      ) : null}
      {!looking && !noKey ? (
        <div className="px-5 pt-2">
          <Button size="sm" variant="ghost" onClick={() => setPicking((open) => !open)}>
            {t("torrentsFeed.panel.pickAnother")}
          </Button>
        </div>
      ) : null}
      {picking ? (
        <PickAnother
          row={row}
          onPick={(choice) => {
            setPicking(false);
            onPick(choice);
          }}
        />
      ) : null}

      {overview === null || overview === "" ? null : (
        <p className="m-0 mx-5 mt-3 text-(length:--fs-13) leading-normal text-text-2">{overview}</p>
      )}

      {inLibrary === null ? null : (
        <p className="m-0 mx-5 mt-3 flex items-center gap-2 text-(length:--fs-13)">
          <Icon icon={Check} size={16} className="text-ok" />
          <span className="text-text-2">{t("torrentsFeed.panel.inLibrary")}</span>
          {row.kind === "movie" ? (
            <Link
              to="/movies/$movieId"
              params={{ movieId: inLibrary.itemId }}
              className="cv-link truncate"
            >
              {inLibrary.name}
            </Link>
          ) : (
            <Link
              to="/tv/$showId"
              params={{ showId: inLibrary.itemId }}
              className="cv-link truncate"
            >
              {inLibrary.name}
            </Link>
          )}
        </p>
      )}

      {links.length === 0 ? null : (
        <>
          <SectionLabel className="mx-5 mt-4 mb-1.5">{t("torrentsFeed.panel.links")}</SectionLabel>
          <div className="flex flex-wrap gap-1.5 px-5">
            {links.map((link) => (
              <a
                key={link.label}
                href={link.url}
                target="_blank"
                rel="noopener noreferrer"
                className="cv-hoverable inline-flex h-(--h-btn-sm) items-center gap-1 rounded-(--r-6) border border-solid border-line-edge px-2 text-(length:--fs-12)"
                onClick={(event) => {
                  event.preventDefault();
                  void openRatingPage(link.url, link.label);
                }}
              >
                {link.label}
                <Icon icon={ExternalLink} size={12} />
              </a>
            ))}
          </div>
        </>
      )}

      <SectionLabel className="mx-5 mt-4 mb-1.5">
        {t("torrentsFeed.panel.thisRelease")}
      </SectionLabel>
      <div className="flex flex-col gap-2 px-5">
        <p className="cv-selectable m-0 font-mono text-(length:--fs-12) break-all text-text-2">
          {row.release.name}
        </p>
        <p className="m-0 flex flex-wrap items-center gap-x-2 text-(length:--fs-12) text-text-muted">
          <QualityBadge quality={tierOf(row.release)} />
          {row.release.size === null ? null : <FileSize bytes={row.release.size} />}
          {row.release.seeders === null ? null : (
            <span>{t("torrentsFeed.panel.seeders", { count: row.release.seeders })}</span>
          )}
          {sourceNames === "" ? null : <span>{sourceNames}</span>}
          {age === "" ? null : <span>{age}</span>}
        </p>
        <div className="flex flex-wrap items-center gap-2">
          <Button variant="primary" icon={Send} onClick={() => void openRelease(row.release)}>
            {t("torrentsPage.send")}
          </Button>
          {hit === null ? null : (
            <Button icon={Search} onClick={() => onSearch(hit)}>
              {t("torrentsFeed.panel.searchAll")}
            </Button>
          )}
        </div>
      </div>

      {others.length === 0 ? null : (
        <>
          <SectionLabel className="mx-5 mt-4 mb-1.5">
            {t("torrentsFeed.panel.releases")}
          </SectionLabel>
          <ul className="m-0 flex list-none flex-col gap-0.5 px-2.5 pb-6">
            {others.map((other) => (
              <li
                key={other.release.id}
                className="flex items-center gap-2 rounded-(--r-6) px-2.5 py-1.5"
              >
                <QualityBadge quality={tierOf(other.release)} />
                <button
                  type="button"
                  className="cv-link min-w-0 flex-1 truncate text-left font-mono text-(length:--fs-12)"
                  title={other.release.name}
                  onClick={() => onShowRelease(other)}
                >
                  {other.release.name}
                </button>
                {other.release.size === null ? null : (
                  <span className="shrink-0 text-(length:--fs-12) text-text-muted">
                    <FileSize bytes={other.release.size} />
                  </span>
                )}
                <IconButton
                  icon={Send}
                  size="sm"
                  aria-label={t("torrentsPage.send")}
                  onClick={() => void openRelease(other.release)}
                />
              </li>
            ))}
          </ul>
        </>
      )}
      <div className="h-6 shrink-0" />
    </aside>
  );
}
