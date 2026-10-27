import { useQuery } from "@tanstack/react-query";
import { Link, useSearch } from "@tanstack/react-router";

import { Button } from "../../components/Button";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { CachedImage } from "../images/CachedImage";
import { currentCollectionBackend } from "../library/collectionBackend";
import { isImageKey } from "../images/imageUrl";
import { subjectName, useTitleSubject } from "../scrape/titleSubject";
import type { TitleSubject } from "../scrape/titleSubject";
import { usePickerClose } from "../scrape/usePickerClose";
import { useEscapeLayer } from "../shell/escStack";
import { ArtworkPicker } from "./ArtworkPicker";
import { artworkIdsOf } from "./artworkSearch";
import type { ArtworkSearch } from "./artworkSearch";
import { ArtworkBatch } from "./ArtworkBatch";

/** The small poster at the left of a picker page's header. */
function HeaderPoster({ subject }: { subject: Pick<TitleSubject, "poster" | "title"> }) {
  const key = subject.poster?.key ?? null;
  return (
    <span className="relative h-[39px] w-[26px] shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active">
      {key !== null && isImageKey(key) ? (
        <CachedImage imageKey={key} size="list" alt="" className="size-full" />
      ) : (
        <PlaceholderPoster title={subject.title} className="size-full" />
      )}
    </span>
  );
}

interface HeaderProps {
  subject: Pick<TitleSubject, "poster" | "title" | "year">;
  onDone: () => void;
}

/** The header of the artwork picker: the title's poster, what is picked and for whom, and Done. */
function PickerHeader({ subject, onDone }: HeaderProps) {
  return (
    <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
      <span aria-hidden="true" className="absolute right-6 bottom-0 left-6 h-px bg-line-section" />
      <HeaderPoster subject={subject} />
      <h1
        title={t("artworkPicker.title.tip")}
        className="m-0 min-w-0 flex-1 truncate text-(length:--fs-16) font-(weight:--fw-700)"
      >
        {t("artworkPicker.title", { name: subjectName(subject) })}
      </h1>
      <Button variant="ghost" shortcut="Esc" onClick={onDone}>
        {t("artworkPicker.done")}
      </Button>
    </div>
  );
}

function NotFound() {
  return (
    <section className="flex flex-col items-start gap-3 px-6 pt-6">
      <h1 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
        {t("artworkPicker.loadFailed")}
      </h1>
      <Link to="/library" className="cv-link">
        {t("artworkPicker.back")}
      </Link>
    </section>
  );
}

function Loading() {
  return (
    <section aria-busy="true" className="flex flex-col gap-3 px-6 pt-4">
      <Skeleton shape="line" width="320px" />
      <Skeleton shape="line" width="100%" height="34px" />
    </section>
  );
}

/** `/artwork` opened with nothing to choose artwork for. */
function NoTitles() {
  return (
    <section className="flex flex-col items-start gap-3 px-6 pt-6">
      <h1 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
        {t("artworkPicker.nothing")}
      </h1>
      <Link to="/library" className="cv-link">
        {t("artworkPicker.back")}
      </Link>
    </section>
  );
}

/** The picker for one title. */
function SingleArtwork({ id, type }: { id: string; type: string }) {
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const subject = useTitleSubject(id);
  if (subject.data === undefined) return subject.isError ? <NotFound /> : <Loading />;
  const name = subjectName(subject.data);
  return (
    <section
      aria-label={t("artworkPicker.region")}
      className="relative flex h-full min-h-0 flex-col"
    >
      <ArtworkPicker
        key={id}
        itemId={id}
        title={name}
        initialSlot={type}
        header={<PickerHeader subject={subject.data} onDone={close} />}
        onCancel={close}
      />
    </section>
  );
}

/** The poster and the fanart are what a collection has (spec `artwork` "Collection artwork can be changed"). */
const COLLECTION_SLOTS: readonly string[] = ["poster", "fanart"];

/** The picker for a collection: its poster and fanart, written to the collection's artwork location. */
function CollectionArtwork({ id, type }: { id: string; type: string }) {
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const collection = useQuery({
    queryKey: ["library", "collection", id] as const,
    queryFn: () => currentCollectionBackend().get(id),
    staleTime: 30_000,
  });
  if (collection.data === undefined) return collection.isError ? <NotFound /> : <Loading />;
  const subject = { title: collection.data.title, year: 0, poster: collection.data.poster };
  return (
    <section
      aria-label={t("artworkPicker.collection.region")}
      className="relative flex h-full min-h-0 flex-col"
    >
      <ArtworkPicker
        key={id}
        itemId={id}
        title={collection.data.title}
        initialSlot={COLLECTION_SLOTS.includes(type) ? type : "poster"}
        onlySlots={COLLECTION_SLOTS}
        header={<PickerHeader subject={subject} onDone={close} />}
        onCancel={close}
      />
    </section>
  );
}

/**
 * The artwork picker (`/artwork`): `?ids=` names one title
 * or several to step through, `?type=` the artwork type it opens on, `?collection=` a collection.
 */
export function ArtworkPage() {
  const search = useSearch({ strict: false }) as ArtworkSearch;
  const ids = artworkIdsOf(search);
  const type = search.type ?? "poster";
  const [only] = ids;
  if (search.collection !== undefined) {
    return <CollectionArtwork key={search.collection} id={search.collection} type={type} />;
  }
  if (ids.length === 1 && only !== undefined)
    return <SingleArtwork key={only} id={only} type={type} />;
  if (ids.length > 1) return <ArtworkBatch key={ids.join(",")} ids={ids} type={type} />;
  return <NoTitles />;
}
