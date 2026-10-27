import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Link, useParams } from "@tanstack/react-router";
import { useCallback, useEffect, useId, useState } from "react";

import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useClearNewOnOpen } from "../curation/newFlagActions";
import { displayTitle, usePreferOriginalTitle } from "../library/displayTitle";
import { DetailRequestError, currentDetailBackend } from "../detail/detailBackend";
import type { MovieDetail } from "../detail/detailBackend";
import { filesStateOf } from "../detail/filesState";
import { metaLine } from "../detail/detailFormat";
import { useDetailWithPortraits } from "../detail/useCastPortraits";
import { runNfoAction, selectionScope } from "../nfo/nfoActions";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { FilesNotice } from "../title/FilesNotice";
import { RatingStrip } from "../title/RatingStrip";
import { TitleActionBar } from "../title/TitleActionBar";
import { TitleHeader } from "../title/TitleHeader";
import { useTitleActions } from "../title/titleActions";
import { idMap } from "../ratings/ratingLinks";
import { ArtworkStrip } from "./ArtworkStrip";
import { CastSection } from "./CastSection";
import { MovieFacts } from "./MovieFacts";
import { MovieTags } from "./MovieTags";
import { NfoPanel } from "./NfoPanel";
import { TechnicalCard } from "./TechnicalCard";
import { TechnicalDetails } from "./TechnicalDetails";
import { currentRenameHistoryBackend, useRenamedFrom } from "./renamedFrom";
import { useMediaInfoRead } from "./useMediaInfoRead";

/** Scrolls an opened block into view; a window without layout (a test) has nothing to scroll. */
function useRevealOnOpen(id: string, open: boolean) {
  useEffect(() => {
    if (!open) return;
    document.getElementById(id)?.scrollIntoView?.({ block: "nearest" });
  }, [id, open]);
}

/** The loaded Movie page: header, then plot, facts and cast beside the Technical card, then artwork. */
function MovieView({ detail }: { detail: MovieDetail }) {
  const format = useFormat();
  const heading = displayTitle(detail.title, detail.original_title, usePreferOriginalTitle());
  const queryClient = useQueryClient();
  const backend = currentDetailBackend();
  const detailsId = useId();
  const nfoId = useId();
  const [detailsOpen, setDetailsOpen] = useState(false);
  const [nfoOpen, setNfoOpen] = useState(false);
  useRevealOnOpen(detailsId, detailsOpen);
  useRevealOnOpen(nfoId, nfoOpen);

  const nfo = useQuery({
    queryKey: ["movie-nfo", detail.id],
    queryFn: () => backend.nfo(detail.id),
    staleTime: 0,
  });
  const read = useMediaInfoRead([["movie-detail", detail.id]]);
  const renamed = useRenamedFrom(detail.id);
  const filesState = filesStateOf(detail.availability);

  const showNfo = useCallback(() => setNfoOpen(true), []);
  const actions = useTitleActions({
    id: detail.id,
    kind: "movie",
    title: detail.title,
    folder: detail.folder,
    availability: detail.availability,
    playId: detail.id,
    onShowNfo: showNfo,
  });

  const undoRename = useCallback(() => {
    currentRenameHistoryBackend()
      .undo(detail.id)
      .then(() => {
        void queryClient.invalidateQueries();
        showToast({ message: t("movie.tech.undone", { title: detail.title }) });
      })
      .catch((error: unknown) =>
        notify({
          severity: "error",
          title: t("movie.tech.undoFailed", { title: detail.title }),
          detail: error instanceof Error ? error.message : String(error),
        }),
      );
  }, [detail.id, detail.title, queryClient]);

  const collection = detail.collection;
  const context =
    collection === null
      ? undefined
      : t("movie.header.collection", {
          name: collection.name,
          owned: collection.owned,
          total: collection.total,
        });

  return (
    <article aria-label={heading} className="relative pb-10 [&>*]:relative">
      <TitleHeader
        itemId={detail.id}
        title={heading}
        poster={detail.poster}
        backdrop={detail.backdrop}
        lifecycle={detail.lifecycle}
        context={context}
        meta={metaLine(detail)}
        tagline={detail.tagline}
        ratings={
          <RatingStrip
            ratings={detail.ratings}
            link={{
              kind: "movie",
              ids: idMap(detail.ids),
              title: detail.title,
              year: detail.year,
            }}
          />
        }
        actions={<TitleActionBar actions={actions} label={t("movie.actions.label")} />}
      />

      <div className="relative flex gap-7 px-(--inset) pt-1.5">
        <div className="flex min-w-0 flex-[1.25] flex-col gap-6">
          {filesState === null ? null : <FilesNotice state={filesState} folder={detail.folder} />}
          {detail.plot === "" ? (
            <p className="cv-plot text-text-muted">{t("movie.plot.none")}</p>
          ) : (
            <p className="cv-plot">{detail.plot}</p>
          )}
          <MovieFacts detail={detail} format={format} />
          <MovieTags tags={detail.tags} />
          <CastSection credits={detail.credits} />
        </div>
        <div className="min-w-0 flex-1">
          <TechnicalCard
            detail={detail}
            nfo={nfo.data}
            expanded={detailsOpen}
            detailsId={detailsId}
            onToggleDetails={() => setDetailsOpen((value) => !value)}
            onShowNfo={() => setNfoOpen((value) => !value)}
            read={read}
            renamed={renamed.data ?? null}
            onUndoRename={undoRename}
          />
          <NfoPanel
            id={nfoId}
            open={nfoOpen}
            nfo={nfo.data}
            error={nfo.error}
            onHide={() => setNfoOpen(false)}
            onRewrite={() => void runNfoAction("rewrite", selectionScope([detail.id]))}
          />
        </div>
      </div>

      <TechnicalDetails
        id={detailsId}
        detail={detail}
        open={detailsOpen}
        onHide={() => setDetailsOpen(false)}
      />
      <ArtworkStrip itemId={detail.id} artwork={detail.artwork} />
    </article>
  );
}

/** Reserved shapes while the movie loads: nothing moves when it arrives (skeletons after 150 ms). */
function MovieSkeleton() {
  return (
    <section aria-busy="true" aria-label={t("detail.loading")} className="px-(--inset) pt-[30px]">
      <div className="flex items-end gap-7">
        <Skeleton shape="tile" aspect="2 / 3" className="w-[172px] shrink-0" />
        <div className="flex flex-1 flex-col gap-3">
          <Skeleton shape="line" width="40%" />
          <Skeleton shape="line" width="55%" height="32px" />
          <Skeleton shape="line" width="30%" />
        </div>
      </div>
    </section>
  );
}

/** The route `/movies/$movieId`. */
export function MoviePage() {
  const { movieId } = useParams({ strict: false }) as { movieId?: string };
  const id = movieId ?? "";
  const backend = currentDetailBackend();
  const query = useQuery({
    queryKey: ["movie-detail", id],
    queryFn: () => backend.movie(id),
    // The movie may have changed since it was last open.
    staleTime: 0,
  });
  const detail = useDetailWithPortraits(query.data);
  // A title is NEW until it is opened.
  useClearNewOnOpen(id);

  if (query.data !== undefined) return <MovieView detail={detail ?? query.data} key={id} />;
  if (query.isPending) return <MovieSkeleton />;

  const missing = query.error instanceof DetailRequestError && query.error.code === "not_found";
  return (
    <section className="flex flex-col items-start gap-2 px-(--inset) pt-6">
      <h1 className="type-page-title m-0">
        {missing ? t("detail.notFound.title") : t("detail.error.title")}
      </h1>
      <p role="alert" className="m-0 text-text-2">
        {missing ? t("detail.notFound.body") : query.error.message}
      </p>
      <div className="flex gap-4">
        {missing ? null : (
          <button type="button" onClick={() => void query.refetch()} className="cv-link">
            {t("detail.error.retry")}
          </button>
        )}
        <Link to="/library" search={{ type: "movies" }} className="cv-link">
          {t("movie.backToLibrary")}
        </Link>
      </div>
    </section>
  );
}
