import { syntheticMovieDetail } from "../detail/syntheticDetail";
import { syntheticEpisodeDetail, syntheticShowDetail } from "../tv/syntheticTv";
import { EditRequestError } from "./editBackend";
import type {
  EditBackend,
  EditField,
  EditKind,
  EditTarget,
  EditValue,
  ItemEdit,
} from "./editBackend";
import { sameValue } from "./editModel";

/**
 * An edit backend for a plain browser and for tests: the fields of the fixture movie, show and
 * episode live in memory, and a save changes them until the page is reloaded. It follows the
 * rules the desktop side checks (a title is needed, the year and the date must make sense).
 */

type Seed = readonly [key: string, value: EditValue];

const text = (value: string): EditValue => ({ type: "text", value });
const num = (value: number | null): EditValue => ({ type: "number", value });
const list = (values: readonly string[]): EditValue => ({ type: "list", values: [...values] });
const date = (value: string | null): EditValue => ({ type: "date", value });
const empty = (): EditValue => text("");

/** The lock key of a field, the way the desktop side maps it. */
function lockOf(key: string): string | null {
  const keys: Readonly<Record<string, string>> = {
    title: "title",
    original_title: "original_title",
    tagline: "tagline",
    plot: "plot",
    year: "year",
    release_date: "release_date",
    first_aired: "release_date",
    status: "status",
    top250: "top250",
    runtime: "runtime",
    certification: "certification",
    genres: "genres",
    spoken_languages: "spoken_languages",
    countries: "country",
    country: "country",
    studios: "studio",
    tags: "tags",
    cast: "actors",
    crew: "crew",
    trailers: "trailer",
    ratings: "rating",
    collection: "collection",
  };
  if (key.startsWith("artwork.")) return key.slice("artwork.".length);
  return keys[key] ?? null;
}

interface Credit {
  name: string;
  role: string;
  kind: string;
}

function people(credits: readonly Credit[], cast: boolean): EditValue {
  return {
    type: "people",
    people: credits
      .filter((c) => (c.kind === "actor" || c.kind === "guest") === cast)
      .map((c) => ({ kind: c.kind, name: c.name, role: c.role })),
  };
}

function ids(pairs: readonly { provider: string; value: string }[]): EditValue {
  return { type: "pairs", pairs: pairs.map((p) => ({ key: p.provider, value: p.value })) };
}

function ratings(
  rows: readonly { source: string; value: number | null; max: number; votes: number }[],
): EditValue {
  return {
    type: "ratings",
    ratings: rows
      .filter((row) => row.value !== null && row.source !== "user")
      .map((row) => ({
        source: row.source,
        value: row.value ?? 0,
        max: row.max,
        votes: row.votes,
      })),
  };
}

/** The collections the fixture library holds. */
const COLLECTIONS: readonly string[] = ["Alien Collection", "Dune Collection"];

function artwork(slots: readonly string[]): Seed[] {
  return slots.map((slot) => [`artwork.${slot}`, empty()] as const);
}

function movieSeed(): Seed[] {
  const d = syntheticMovieDetail();
  return [
    ["title", text(d.title)],
    ["original_title", text(d.original_title)],
    ["sort_title", empty()],
    ["tagline", text(d.tagline)],
    ["plot", text(d.plot)],
    ["outline", empty()],
    ["year", num(d.year)],
    ["release_date", date(d.release_date)],
    ["runtime", num(d.runtime_minutes)],
    ["certification", text(d.certification)],
    ["genres", list(d.genres)],
    ["studios", list(d.studios)],
    ["countries", list(d.countries)],
    ["spoken_languages", list(d.languages)],
    ["edition", text(d.edition)],
    ["media_source", text(d.media_source)],
    ["video_in_3d", { type: "flag", value: false }],
    ["tags", list(d.tags)],
    ["note", text(d.note)],
    ["user_rating", num(d.user_rating)],
    ["ratings", ratings(d.ratings)],
    ["collection", text(d.collection?.name ?? "")],
    ["play_count", num(d.play_count > 0 ? d.play_count : null)],
    ["last_watched", { type: "date", value: null }],
    ["top250", num(d.top250 > 0 ? d.top250 : null)],
    ["ids", ids(d.ids)],
    ["cast", people(d.credits, true)],
    ["crew", people(d.credits, false)],
    [
      "trailers",
      {
        type: "trailers",
        trailers: d.trailers
          .filter((tr) => tr.url !== "")
          .map((tr) => ({
            name: tr.name,
            url: tr.url,
            quality: tr.quality,
            language: tr.language,
            in_nfo: true,
          })),
      },
    ],
    ...artwork(["poster", "fanart", "banner", "clearlogo", "clearart", "disc", "thumb", "keyart"]),
  ];
}

function showSeed(): Seed[] {
  const d = syntheticShowDetail();
  const seasons = d.seasons.flatMap((season) => [
    [`season_name.${season.number}`, empty()] as const,
    [`season_overview.${season.number}`, empty()] as const,
  ]);
  return [
    ["title", text(d.title)],
    ["original_title", text(d.original_title)],
    ["sort_title", empty()],
    ["tagline", text(d.tagline)],
    ["plot", text(d.plot)],
    ["outline", empty()],
    ["year", num(d.year)],
    ["first_aired", date(d.first_aired)],
    ["status", text(d.status)],
    ["runtime", num(d.runtime_minutes)],
    ["certification", text(d.certification)],
    ["genres", list(d.genres)],
    ["studios", list(d.studios)],
    ["country", text(d.countries[0] ?? "")],
    ["tags", list(d.tags)],
    ["note", text(d.note)],
    ["user_rating", num(d.user_rating)],
    ["ratings", ratings(d.ratings)],
    ["top250", num(d.top250 > 0 ? d.top250 : null)],
    ["ids", ids(d.ids)],
    ["cast", people(d.credits, true)],
    ["crew", people(d.credits, false)],
    ["trailers", { type: "trailers", trailers: [] }],
    ...artwork(["poster", "fanart", "banner", "clearlogo", "clearart", "thumb"]),
    ...seasons,
  ];
}

function episodeSeed(id: string): Seed[] {
  const d = syntheticEpisodeDetail(id);
  return [
    ["title", text(d.title)],
    ["original_title", text(d.original_title)],
    ["plot", text(d.plot)],
    ["first_aired", date(d.first_aired)],
    ["runtime", num(d.runtime_minutes)],
    ["edition", empty()],
    ["media_source", empty()],
    ["studios", list(d.studios)],
    ["tags", list(d.tags)],
    ["note", text(d.note)],
    ["user_rating", num(d.user_rating)],
    ["ids", ids(d.ids)],
    ["numbers", { type: "numbers", numbers: d.numbers }],
    ["cast", people(d.credits.concat(d.guests), true)],
    ["crew", people(d.credits, false)],
    ...artwork(["thumb"]),
  ];
}

function seed(target: EditTarget): Seed[] {
  const builders: Readonly<Record<EditKind, () => Seed[]>> = {
    movie: movieSeed,
    show: showSeed,
    episode: () => episodeSeed(target.id),
  };
  return builders[target.kind]();
}

/** Why a value is refused, or null when it is fine. */
function refusal(field: EditField): string | null {
  const v = field.value;
  if (field.key === "title" && v.type === "text" && v.value.trim() === "") {
    return "A title is required.";
  }
  if (field.key === "year" && v.type === "number" && v.value !== null) {
    if (!Number.isInteger(v.value) || v.value < 1800 || v.value > 2200) {
      return "Enter a year from 1800 to 2200, or leave it empty.";
    }
  }
  if (v.type === "date" && v.value !== null && !/^\d{4}-\d{2}-\d{2}$/.test(v.value)) {
    return "Enter the date as year-month-day, such as 2024-03-01.";
  }
  if (field.key === "collection" && v.type === "text") {
    const name = v.value.trim();
    if (name !== "" && !COLLECTIONS.some((known) => known.toLowerCase() === name.toLowerCase())) {
      return `There is no collection called "${name}".`;
    }
  }
  if (field.key.startsWith("artwork.") && v.type === "text") {
    const address = v.value.trim();
    if (address !== "" && !/^https?:\/\//.test(address)) {
      return "Enter a web address that starts with http:// or https://.";
    }
  }
  return null;
}

interface Stored {
  fields: Map<string, EditField>;
  locked: Set<string>;
  itemLocked: boolean;
}

export function createSyntheticEditBackend(): EditBackend {
  const items = new Map<string, Stored>();
  const find = (target: EditTarget): Stored => {
    const key = `${target.kind}:${target.id}`;
    let stored = items.get(key);
    if (stored === undefined) {
      stored = {
        fields: new Map(
          seed(target).map(([k, v]) => [k, { key: k, value: v, lock: lockOf(k) }] as const),
        ),
        locked: new Set(),
        itemLocked: false,
      };
      items.set(key, stored);
    }
    return stored;
  };
  return {
    load: (target) =>
      Promise.resolve().then((): ItemEdit => {
        const stored = find(target);
        return {
          kind: target.kind,
          fields: [...stored.fields.values()],
          locked_fields: [...stored.locked],
          item_locked: stored.itemLocked,
          collections: target.kind === "movie" ? [...COLLECTIONS] : [],
        };
      }),
    save: (target, changes) =>
      Promise.resolve().then(() => {
        const stored = find(target);
        for (const change of changes) {
          const reason = refusal(change);
          if (reason !== null) throw new EditRequestError("invalid", reason, change.key);
        }
        const changed: string[] = [];
        const previous: EditField[] = [];
        for (const change of changes) {
          const before = stored.fields.get(change.key);
          if (before !== undefined && sameValue(before.value, change.value)) continue;
          previous.push(before ?? { key: change.key, value: empty(), lock: null });
          stored.fields.set(change.key, {
            key: change.key,
            value: change.value,
            lock: lockOf(change.key),
          });
          changed.push(change.key);
        }
        return {
          changed,
          previous,
          nfo_files_written: changed.length > 0 ? 1 : 0,
          nfo_problem: null,
        };
      }),
    setFieldLock: (target, lockKey, locked) =>
      Promise.resolve().then(() => {
        const stored = find(target);
        if (locked) stored.locked.add(lockKey);
        else stored.locked.delete(lockKey);
      }),
    setItemLock: (target, locked) =>
      Promise.resolve().then(() => {
        find(target).itemLocked = locked;
      }),
  };
}
