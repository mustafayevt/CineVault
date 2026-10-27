import { useQueryClient } from "@tanstack/react-query";
import { useNavigate, useParams } from "@tanstack/react-router";
import { ArrowLeft } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ReactNode } from "react";

import { Card } from "../../components/Card";
import { IconButton } from "../../components/IconButton";
import { SectionLabel } from "../../components/SectionLabel";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import {
  ChoiceControl,
  DateControl,
  FlagControl,
  IdsControl,
  ListControl,
  NumberControl,
  PeopleControl,
  RatingsControl,
  Row,
  TextAreaControl,
  TextControl,
} from "./EditControls";
import { EditFootBar } from "./EditFootBar";
import { SessionContext } from "./formContext";
import { NUMBER_RULES, YEAR_RULE, seasonNameKey, seasonOverviewKey } from "./editModel";
import type { EditKind } from "./editBackend";
import {
  allCountryNames,
  allLanguageNames,
  countryName,
  languageName,
  resolveCountry,
  resolveLanguage,
} from "./names";
import { CERTIFICATIONS, EDITIONS, GENRES, SOURCES } from "./suggestions";
import { useItemEdit } from "./useItemEdit";
import type { EditSession } from "./useItemEdit";

/** Artwork types whose web address can be edited, with the name the strip uses. */
const ARTWORK_LABELS: Readonly<Record<string, MessageKey>> = {
  poster: "movie.art.poster",
  fanart: "movie.art.fanart",
  clearlogo: "movie.art.clearlogo",
  clearart: "movie.art.clearart",
  disc: "movie.art.disc",
  banner: "movie.art.banner",
  keyart: "movie.art.keyart",
  thumb: "movie.art.thumb",
};

function Section({ title, children }: { title: string; children: ReactNode }) {
  return (
    <Card>
      <SectionLabel>{title}</SectionLabel>
      <div className="flex flex-col gap-3.5">{children}</div>
    </Card>
  );
}

/** The sections of the form, each only when the item has fields for it. */
function Form({ session, kind }: { session: EditSession; kind: EditKind }) {
  const has = (key: string) => session.base.has(key);
  const any = (...keys: string[]) => keys.some(has);
  const slots = [...session.base.keys()]
    .filter((key) => key.startsWith("artwork."))
    .map((key) => key.slice("artwork.".length));
  const seasons = [...session.base.keys()]
    .map((key) => /^season_name\.(\d+)$/.exec(key)?.[1])
    .filter((n): n is string => n !== undefined)
    .map(Number)
    .sort((a, b) => a - b);
  return (
    <div className="flex flex-col gap-4">
      {any("title", "original_title", "sort_title", "tagline") && (
        <Section title={t("editDetails.section.titles")}>
          <Row>
            {has("title") && <TextControl fieldKey="title" label={t("edit.field.title")} />}
            {has("original_title") && (
              <TextControl fieldKey="original_title" label={t("edit.field.original_title")} />
            )}
            {has("sort_title") && (
              <TextControl fieldKey="sort_title" label={t("edit.field.sort_title")} />
            )}
            {has("tagline") && <TextControl fieldKey="tagline" label={t("edit.field.tagline")} />}
          </Row>
        </Section>
      )}
      {any("plot", "outline") && (
        <Section title={t("editDetails.section.plot")}>
          {has("plot") && <TextAreaControl fieldKey="plot" label={t("edit.field.plot")} rows={5} />}
          {has("outline") && (
            <TextAreaControl fieldKey="outline" label={t("edit.field.outline")} rows={2} />
          )}
        </Section>
      )}
      {any(
        "year",
        "release_date",
        "first_aired",
        "runtime",
        "status",
        "certification",
        "edition",
        "media_source",
        "video_in_3d",
      ) && (
        <Section title={t("editDetails.section.release")}>
          <Row>
            {has("year") && (
              <NumberControl
                fieldKey="year"
                label={t("edit.field.year")}
                min={YEAR_RULE.min}
                max={YEAR_RULE.max}
              />
            )}
            {has("release_date") && (
              <DateControl fieldKey="release_date" label={t("edit.field.release_date")} />
            )}
            {has("first_aired") && (
              <DateControl fieldKey="first_aired" label={t("edit.field.first_aired")} />
            )}
            {has("status") && (
              <ChoiceControl
                fieldKey="status"
                label={t("edit.field.status")}
                options={[
                  { value: "unknown", label: t("tv.status.unknown") },
                  { value: "continuing", label: t("tv.status.continuing") },
                  { value: "ended", label: t("tv.status.ended") },
                  { value: "canceled", label: t("tv.status.canceled") },
                ]}
              />
            )}
            {has("runtime") && (
              <NumberControl
                fieldKey="runtime"
                label={t("edit.field.runtime")}
                min={NUMBER_RULES.runtime?.min ?? 0}
                unit={t("edit.unit.minutes")}
              />
            )}
            {has("certification") && (
              <TextControl
                fieldKey="certification"
                label={t("edit.field.certification")}
                suggestions={CERTIFICATIONS}
              />
            )}
            {has("edition") && (
              <TextControl
                fieldKey="edition"
                label={t("edit.field.edition")}
                suggestions={EDITIONS}
                pretty
              />
            )}
            {has("media_source") && (
              <TextControl
                fieldKey="media_source"
                label={t("edit.field.media_source")}
                suggestions={SOURCES}
                pretty
              />
            )}
            {has("video_in_3d") && (
              <FlagControl fieldKey="video_in_3d" label={t("edit.field.video_in_3d")} />
            )}
          </Row>
        </Section>
      )}
      {any("genres", "studios", "countries", "country", "spoken_languages") && (
        <Section title={t("editDetails.section.classification")}>
          {has("genres") && (
            <ListControl fieldKey="genres" label={t("edit.field.genres")} suggestions={GENRES} />
          )}
          {has("studios") && <ListControl fieldKey="studios" label={t("edit.field.studios")} />}
          {has("countries") && (
            <ListControl
              fieldKey="countries"
              label={t("edit.field.countries")}
              suggestions={allCountryNames()}
              display={countryName}
              normalize={resolveCountry}
            />
          )}
          {has("country") && (
            <TextControl
              fieldKey="country"
              label={t("edit.field.country")}
              suggestions={allCountryNames()}
            />
          )}
          {has("spoken_languages") && (
            <ListControl
              fieldKey="spoken_languages"
              label={t("edit.field.spoken_languages")}
              suggestions={allLanguageNames()}
              display={languageName}
              normalize={resolveLanguage}
            />
          )}
        </Section>
      )}
      {any("ids", "ratings", "user_rating", "top250", "collection", "note") && (
        <Section title={t("editDetails.section.identity")}>
          {has("ids") && <IdsControl fieldKey="ids" label={t("edit.field.ids")} />}
          {has("ratings") && <RatingsControl fieldKey="ratings" label={t("edit.field.ratings")} />}
          <Row>
            {has("user_rating") && (
              <NumberControl
                fieldKey="user_rating"
                label={t("edit.field.user_rating")}
                min={0.5}
                max={10}
                step={0.5}
                unit={t("edit.unit.outOfTen")}
              />
            )}
            {has("top250") && (
              <NumberControl fieldKey="top250" label={t("edit.field.top250")} min={0} max={250} />
            )}
            {has("collection") && kind === "movie" && (
              <ChoiceControl
                fieldKey="collection"
                label={t("edit.field.collection")}
                options={[
                  { value: "", label: t("edit.collection.none") },
                  ...session.collections.map((name) => ({ value: name, label: name })),
                ]}
              />
            )}
          </Row>
          {has("note") && <TextControl fieldKey="note" label={t("edit.field.note")} />}
        </Section>
      )}
      {any("cast", "crew") && (
        <Section title={t("editDetails.section.people")}>
          {has("cast") && <PeopleControl fieldKey="cast" label={t("edit.field.cast")} />}
          {has("crew") && <PeopleControl fieldKey="crew" label={t("edit.field.crew")} crew />}
        </Section>
      )}
      {slots.length > 0 && (
        <Section title={t("editDetails.section.artwork")}>
          <p className="type-small m-0 -mt-1 text-text-muted">{t("edit.artwork.help")}</p>
          {slots.map((slot) => {
            const key = ARTWORK_LABELS[slot];
            return (
              <TextControl
                key={slot}
                fieldKey={`artwork.${slot}`}
                label={key === undefined ? slot : t(key)}
                placeholder="https://"
              />
            );
          })}
        </Section>
      )}
      {seasons.length > 0 && (
        <Section title={t("editDetails.section.seasons")}>
          {seasons.map((season) => (
            <div
              key={season}
              className="flex flex-col gap-3 border-t border-solid border-line-row pt-3 first:border-t-0 first:pt-0"
            >
              <b className="type-card-title">
                {season === 0
                  ? t("tv.season.specials")
                  : t("tv.season.name", { number: String(season) })}
              </b>
              <TextControl fieldKey={seasonNameKey(season)} label={t("edit.season.name")} />
              <TextAreaControl
                fieldKey={seasonOverviewKey(season)}
                label={t("edit.season.overview")}
                rows={3}
              />
            </div>
          ))}
        </Section>
      )}
    </div>
  );
}

interface EditDetailsProps {
  kind: "movie" | "show";
  id: string;
}

/**
 * Edit details (spec `metadata-editing`): every persisted field of a title on one page, grouped in
 * sections of cards and fields, with no dialog. Nothing is written until Save (Ctrl+S); the foot
 * bar counts the unsaved changes and offers Save and Discard, and Back asks there before leaving
 * with changes, so nothing is lost silently. Saving rewrites the NFO, shows the title page again
 * and leaves an Undo toast. Tags and trailers are kept by the backend and never shown here.
 */
function EditDetails({ kind, id }: EditDetailsProps) {
  const navigate = useNavigate();
  // The toast names the title: the page it came from has it in the cache.
  const queryClient = useQueryClient();
  const cached = queryClient.getQueryData<{ title?: string }>([
    kind === "movie" ? "movie-detail" : "tv-show",
    id,
  ]);
  const session = useItemEdit({ id, kind }, cached?.title ?? t("editDetails.thisTitle"), {
    startEditing: true,
  });
  const savedTitle = session.base.get("title");
  const title = savedTitle?.value.type === "text" ? savedTitle.value.value : "";

  const target = kind === "movie" ? `/movies/${id}` : `/tv/${id}`;
  const back = () =>
    void (kind === "movie"
      ? navigate({ to: "/movies/$movieId", params: { movieId: id } })
      : navigate({ to: "/tv/$showId", params: { showId: id } }));

  // After a save the page goes back to the title: once the draft is clear, so the guard is open.
  const [saved, setSaved] = useState(false);
  useEffect(() => {
    if (saved && !session.dirty) back();
    // `back` only closes over the route and ids.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [saved, session.dirty]);
  const save = async () => {
    if (await session.save()) setSaved(true);
  };

  // Ctrl+S saves while there is something to save.
  const latest = useRef({ dirty: session.dirty, save });
  useEffect(() => {
    latest.current = { dirty: session.dirty, save };
  });
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "s") return;
      event.preventDefault();
      if (latest.current.dirty) void latest.current.save();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <SessionContext value={session}>
      <section
        aria-label={t("editDetails.title")}
        data-target={target}
        className="flex min-h-full flex-col"
      >
        <div className="flex flex-1 flex-col gap-4 px-(--inset) pt-4 pb-6">
          <div className="flex items-center gap-3">
            <IconButton icon={ArrowLeft} aria-label={t("editDetails.back")} onClick={back} />
            <div className="min-w-0">
              <h1 className="type-page-title m-0">{t("editDetails.title")}</h1>
              {title === "" ? null : (
                <div className="truncate text-(length:--fs-12) text-text-muted">{title}</div>
              )}
            </div>
          </div>
          <div className="max-w-[820px]">
            {session.loadError !== null ? (
              <p role="alert" className="m-0 text-warn">
                {t("edit.loadFailed", { reason: session.loadError })}
              </p>
            ) : session.loading || session.base.size === 0 ? (
              <div aria-busy="true" aria-label={t("edit.loading")} className="flex flex-col gap-4">
                <Skeleton shape="tile" aspect="6 / 1" />
                <Skeleton shape="tile" aspect="4 / 1" />
                <Skeleton shape="tile" aspect="4 / 1" />
              </div>
            ) : (
              <Form session={session} kind={kind} />
            )}
          </div>
        </div>
        <EditFootBar
          session={session}
          onSave={() => void save()}
          className={cx("sticky bottom-0")}
        />
      </section>
    </SessionContext>
  );
}

/** The route `/movies/$movieId/edit`. */
export function MovieEditPage() {
  const { movieId } = useParams({ strict: false }) as { movieId?: string };
  const id = movieId ?? "";
  return <EditDetails key={id} kind="movie" id={id} />;
}

/** The route `/tv/$showId/edit`. */
export function ShowEditPage() {
  const { showId } = useParams({ strict: false }) as { showId?: string };
  const id = showId ?? "";
  return <EditDetails key={id} kind="show" id={id} />;
}
