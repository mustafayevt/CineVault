import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "@tanstack/react-router";
import { Check, Image } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { SectionLabel } from "../../components/SectionLabel";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { currentCollectionBackend } from "./collectionBackend";
import type { CollectionPage } from "./collectionBackend";
import type { LibraryRow } from "./libraryBackend";

interface PosterBoxProps {
  poster: CollectionPage["poster"];
  title: string;
  size: "list" | "hero";
  className?: string;
}

function PosterBox({ poster, title, size, className }: PosterBoxProps) {
  if (poster === null) return <PlaceholderPoster title={title} className={className} />;
  return (
    <CachedImage
      imageKey={poster.key}
      size={size}
      alt=""
      thumbhash={poster.thumbhash}
      ambient={poster.ambient}
      className={className}
    />
  );
}

/** "Alien 1979": what the torrent search starts from for a film the library lacks. */
function searchText(title: string, year: number): string {
  return year > 0 ? `${title} ${year}` : title;
}

interface CollectionPaneProps {
  /** The row of the collection in the list: its name, counts and size. */
  row: LibraryRow;
}

/**
 * The docked collection pane: flat on the card colour with the section divider
 * at its left, 380px. The collection's artwork with Change artwork, its name and "3 of 5 owned ·
 * 103.6 GB", its overview, and "Films in this set": owned films open their page, missing ones are
 * muted with Search torrents (spec library-browsing "Collections view").
 */
export function CollectionPane({ row }: CollectionPaneProps) {
  const navigate = useNavigate();
  const format = useFormat();
  const id = row.card.id;
  const page = useQuery({
    queryKey: ["library", "collection", id] as const,
    queryFn: () => currentCollectionBackend().get(id),
    staleTime: 30_000,
    retry: false,
  });
  const data = page.data;
  const name = data?.title ?? row.card.title;
  const owned = data?.owned ?? row.movies_in_set;
  const total = data?.total ?? row.movies_in_set + row.missing_movies;
  const bytes = row.total_file_size ?? row.file_size ?? 0;
  const line =
    bytes > 0
      ? t("library.collections.ownedSize", { owned, total, size: format.size(bytes) })
      : t("library.collections.owned", { owned, total });
  const poster = data?.poster ?? row.card.poster;

  return (
    <aside
      aria-label={name}
      className="flex w-[380px] shrink-0 flex-col overflow-y-auto border-l border-solid border-line-section bg-surface-card"
    >
      <div className="relative h-[150px] shrink-0 overflow-hidden bg-surface-window">
        {poster !== null && (
          <CachedImage
            imageKey={poster.key}
            size="hero"
            alt=""
            ambient={poster.ambient}
            className="size-full opacity-40"
          />
        )}
        <div className="cv-collection-fade absolute inset-0" />
        <Button
          icon={Image}
          className="absolute top-3.5 right-3.5"
          onClick={() => void navigate({ to: "/artwork", search: { collection: id } as never })}
        >
          {t("library.collections.changeArtwork")}
        </Button>
      </div>
      <div className="relative -mt-[54px] flex items-end gap-3.5 px-5">
        <div className="aspect-[2/3] w-[84px] shrink-0 overflow-hidden rounded-(--r-10) shadow-floating">
          <PosterBox poster={poster} title={name} size="list" className="size-full" />
        </div>
        <div className="min-w-0 pb-1">
          <h2 className="m-0 text-(length:--fs-16) leading-[1.2] font-(weight:--fw-700)">{name}</h2>
          <p className="m-0 text-(length:--fs-12) text-text-muted">{line}</p>
        </div>
      </div>

      {page.isPending ? (
        <div aria-busy="true" className="flex flex-col gap-2 px-5 pt-3.5">
          <Skeleton shape="line" width="90%" />
          <Skeleton shape="line" width="70%" />
        </div>
      ) : page.isError ? (
        <p role="alert" className="m-0 px-5 pt-3.5 text-(length:--fs-13) text-danger">
          {t("library.collections.loadFailed")}
        </p>
      ) : (
        <>
          <p className="m-0 mx-5 mt-3.5 text-(length:--fs-13) leading-normal text-text-2">
            {data?.plot.trim() === "" || data === undefined
              ? t("library.collections.noPlot")
              : data.plot}
          </p>
          <SectionLabel className="mx-5 mt-[18px] mb-1.5">
            {t("library.collections.films")}
          </SectionLabel>
          <ul className="m-0 flex list-none flex-col gap-0.5 px-2.5 pb-6">
            {data?.entries.map((film, index) => {
              const year = film.year > 0 ? ` (${film.year})` : "";
              const thumb = (
                <span className="h-[39px] w-[26px] shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active">
                  {film.poster !== null && (
                    <CachedImage
                      imageKey={film.poster.key}
                      size="list"
                      alt=""
                      thumbhash={film.poster.thumbhash}
                      ambient={film.poster.ambient}
                      className="size-full"
                    />
                  )}
                </span>
              );
              if (film.id === null) {
                return (
                  <li
                    key={`missing-${index}`}
                    data-owned="false"
                    className="flex h-[46px] items-center gap-2.5 rounded-(--r-6) px-2.5"
                  >
                    {thumb}
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-(weight:--fw-500) text-text-disabled">
                        {film.title}
                        {year}
                      </div>
                      <div className="text-(length:--fs-12) text-text-muted">
                        {t("library.collections.notOwned")}
                      </div>
                    </div>
                    <Link
                      to="/torrents"
                      search={{ q: searchText(film.title, film.year) }}
                      className="cv-link shrink-0 text-(length:--fs-12)"
                    >
                      {t("library.collections.searchTorrents")}
                    </Link>
                  </li>
                );
              }
              return (
                <li key={film.id} data-owned="true">
                  <Link
                    to="/movies/$movieId"
                    params={{ movieId: film.id }}
                    className={cx(
                      "cv-hoverable flex h-[46px] items-center gap-2.5 rounded-(--r-6) px-2.5",
                      "text-text no-underline hover:bg-surface-active active:bg-surface-pressed",
                    )}
                  >
                    {thumb}
                    <div className="min-w-0 flex-1">
                      <div className="truncate font-(weight:--fw-500)">
                        {film.title}
                        <span className="text-text-muted">{year}</span>
                      </div>
                      <div className="text-(length:--fs-12) text-text-muted">
                        {t("library.collections.inLibrary")}
                      </div>
                    </div>
                    <Icon icon={Check} size={16} className="shrink-0 text-ok" />
                  </Link>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </aside>
  );
}
