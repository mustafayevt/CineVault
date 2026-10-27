import { useQuery } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useId, useMemo, useState } from "react";
import type { ReactNode } from "react";

import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { PathText } from "../../components/PathText";
import { useRememberOpened } from "../search/recents";
import { useClearNewOnOpen } from "../curation/newFlagActions";
import { displayTitle, usePreferOriginalTitle } from "../library/displayTitle";
import { DetailRequestError } from "../detail/detailBackend";
import { filesStateOf } from "../detail/filesState";
import { useDetailWithPortraits } from "../detail/useCastPortraits";
import { NfoPanel } from "../movie/NfoPanel";
import { runNfoAction, selectionScope } from "../nfo/nfoActions";
import { idMap } from "../ratings/ratingLinks";
import { FilesNotice } from "../title/FilesNotice";
import { RatingStrip } from "../title/RatingStrip";
import { TitleActionBar } from "../title/TitleActionBar";
import { TitleHeader } from "../title/TitleHeader";
import { useTitleActions } from "../title/titleActions";
import { currentTvBackend } from "../tv/tvBackend";
import type { ShowDetail } from "../tv/tvBackend";
import { showMetaLine } from "../tv/tvFormat";
import { SeasonSection } from "./SeasonSection";
import { distinctSize, listedSeasons, openingSeason, playEpisode, showProgress } from "./showModel";

/** The facts at the right of the header: creators, what is on disk, quality, folder and NFO. */
function ShowFacts({ detail, nfoOk }: { detail: ShowDetail; nfoOk: boolean | null }) {
  const format = useFormat();
  const progress = showProgress(detail);
  const size = distinctSize(detail.episodes);
  const creators = detail.credits.filter((credit) => /creat/i.test(credit.role));
  const quality = detail.tech
    .filter((badge) => badge.facet === "video_format" || badge.facet === "hdr_format")
    .map((badge) => badge.label)
    .join(" · ");
  const rows: [string, ReactNode][] = [];
  if (creators.length > 0) {
    rows.push([t("show.facts.createdBy"), format.list(creators.map((credit) => credit.name))]);
  }
  rows.push([
    t("show.facts.onDisk"),
    [
      t("show.facts.episodesOf", { owned: progress.owned, total: progress.total }),
      size === null ? "" : format.size(size),
    ]
      .filter((part) => part !== "")
      .join(" · "),
  ]);
  if (quality !== "") rows.push([t("show.facts.quality"), quality]);
  if (detail.folder !== "") {
    rows.push([
      t("show.facts.folder"),
      <span key="folder" className="type-code cv-selectable flex">
        <PathText path={detail.folder} />
      </span>,
    ]);
  }
  if (nfoOk !== null) {
    rows.push([
      t("show.facts.nfo"),
      <span key="nfo" className={nfoOk ? "text-ok" : "text-warn"}>
        {nfoOk ? t("show.facts.nfoOk") : t("show.facts.nfoDiffers")}
      </span>,
    ]);
  }
  return (
    <dl
      data-testid="show-facts"
      className="m-0 grid grid-cols-[90px_minmax(0,1fr)] gap-y-1.5 text-(length:--fs-13)"
    >
      {rows.map(([label, value]) => (
        <div key={label} className="contents">
          <dt className="text-text-muted">{label}</dt>
          <dd className="m-0 min-w-0">{value}</dd>
        </div>
      ))}
    </dl>
  );
}

/** The loaded TV show page: header, plot, then every season as a section. */
function ShowView({ detail }: { detail: ShowDetail }) {
  const backend = currentTvBackend();
  const heading = displayTitle(detail.title, detail.original_title, usePreferOriginalTitle());
  const nfoId = useId();
  useRememberOpened({
    kind: "show",
    id: detail.id,
    title: heading,
    year: detail.year,
    poster: detail.poster,
  });
  const [nfoOpen, setNfoOpen] = useState(false);
  const { seasons, specials } = useMemo(() => listedSeasons(detail), [detail]);
  const first = openingSeason(seasons);
  const [openSeasons, setOpenSeasons] = useState<ReadonlySet<number>>(
    () => new Set(first === null ? [] : [first]),
  );
  const [openEpisode, setOpenEpisode] = useState<string | null>(null);
  const nfo = useQuery({
    queryKey: ["tv-nfo", detail.id],
    queryFn: () => backend.showNfo(detail.id),
    staleTime: 0,
  });
  useEffect(() => {
    if (nfoOpen) document.getElementById(nfoId)?.scrollIntoView?.({ block: "nearest" });
  }, [nfoOpen, nfoId]);

  const toggleSeason = (number: number) =>
    setOpenSeasons((current) => {
      const next = new Set(current);
      if (next.has(number)) next.delete(number);
      else next.add(number);
      return next;
    });

  const play = playEpisode(detail);
  const showNfo = useCallback(() => setNfoOpen((value) => !value), []);
  const actions = useTitleActions({
    id: detail.id,
    kind: "show",
    title: detail.title,
    folder: detail.folder,
    availability: detail.availability,
    playId: play?.id ?? null,
    onShowNfo: showNfo,
  });
  const filesState = filesStateOf(detail.availability);
  const status =
    detail.status === "continuing" || detail.status === "ended" || detail.status === "canceled"
      ? t(`show.header.status.${detail.status}`)
      : "";

  return (
    <article aria-label={heading} className="relative pb-10 [&>*]:relative">
      <TitleHeader
        itemId={detail.id}
        title={heading}
        poster={detail.poster}
        backdrop={detail.backdrop}
        lifecycle={detail.lifecycle}
        posterWidth={140}
        backdropHeight={340}
        context={status === "" ? t("show.header.kind") : t("show.header.kindStatus", { status })}
        meta={showMetaLine(detail)}
        ratings={
          <RatingStrip
            ratings={detail.ratings}
            link={{ kind: "show", ids: idMap(detail.ids), title: detail.title, year: detail.year }}
          />
        }
        actions={<TitleActionBar actions={actions} label={t("show.actions.label")} />}
        aside={
          <ShowFacts
            detail={detail}
            nfoOk={nfo.data === undefined ? null : nfo.data.sync === "in_sync"}
          />
        }
      />
      <div className="flex max-w-[908px] flex-col gap-4 px-(--inset) pb-[22px]">
        {filesState === null ? null : <FilesNotice state={filesState} folder={detail.folder} />}
        {detail.plot === "" ? null : (
          <p className="m-0 max-w-[860px] text-(length:--fs-14) leading-[1.6] text-text-2">
            {detail.plot}
          </p>
        )}
        <NfoPanel
          id={nfoId}
          open={nfoOpen}
          nfo={nfo.data}
          error={nfo.error}
          onHide={() => setNfoOpen(false)}
          onRewrite={() => void runNfoAction("rewrite", selectionScope([detail.id]))}
        />
      </div>
      <div className="flex flex-col gap-2.5 px-(--inset)">
        {seasons.length === 0 && specials === null ? (
          <p className="m-0 text-text-muted">{t("show.seasons.none")}</p>
        ) : null}
        {seasons.map((season) => (
          <SeasonSection
            key={season.number}
            detail={detail}
            season={season}
            open={openSeasons.has(season.number)}
            onToggle={() => toggleSeason(season.number)}
            openEpisode={openEpisode}
            onOpenEpisode={setOpenEpisode}
          />
        ))}
        {specials === null ? null : (
          <SeasonSection
            detail={detail}
            season={specials}
            open={openSeasons.has(0)}
            onToggle={() => toggleSeason(0)}
            openEpisode={openEpisode}
            onOpenEpisode={setOpenEpisode}
          />
        )}
      </div>
    </article>
  );
}

function ShowSkeleton() {
  return (
    <section aria-busy="true" aria-label={t("tv.loading")} className="px-(--inset) pt-[26px]">
      <div className="flex items-end gap-[26px]">
        <Skeleton shape="tile" aspect="2 / 3" className="w-[140px] shrink-0" />
        <div className="flex flex-1 flex-col gap-3">
          <Skeleton shape="line" width="30%" />
          <Skeleton shape="line" width="45%" height="32px" />
          <Skeleton shape="line" width="55%" />
        </div>
      </div>
    </section>
  );
}

/** The route `/tv/$showId`. */
export function ShowPage() {
  const { showId } = useParams({ strict: false }) as { showId?: string };
  const id = showId ?? "";
  const backend = currentTvBackend();
  const query = useQuery({
    queryKey: ["tv-show", id],
    queryFn: () => backend.show(id),
    staleTime: 0,
  });
  const detail = useDetailWithPortraits(query.data);
  useClearNewOnOpen(id);

  if (query.data !== undefined) return <ShowView detail={detail ?? query.data} key={id} />;
  if (query.isPending) return <ShowSkeleton />;

  const missing = query.error instanceof DetailRequestError && query.error.code === "not_found";
  return (
    <section className="flex flex-col items-start gap-2 px-(--inset) pt-6">
      <h1 className="type-page-title m-0">
        {missing ? t("tv.notFound.title") : t("tv.error.title")}
      </h1>
      <p role="alert" className="m-0 text-text-2">
        {missing ? t("tv.notFound.body") : query.error.message}
      </p>
      <div className="flex gap-4">
        {missing ? null : (
          <button type="button" onClick={() => void query.refetch()} className="cv-link">
            {t("detail.error.retry")}
          </button>
        )}
        <Link to="/library" search={{ type: "tv" }} className="cv-link">
          {t("show.backToLibrary")}
        </Link>
      </div>
    </section>
  );
}
