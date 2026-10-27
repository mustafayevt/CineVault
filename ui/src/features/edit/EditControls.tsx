import { ArrowDown, ArrowUp, Calendar, Plus, X } from "lucide-react";
import { useId, useState } from "react";
import type { KeyboardEvent, ReactNode } from "react";

import type { EditPairDto, EditPersonDto, EditRatingDto } from "../../bindings";
import { Button } from "../../components/Button";
import { Field } from "../../components/Field";
import { IconButton } from "../../components/IconButton";
import { Select } from "../../components/Select";
import { Switch } from "../../components/Switch";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { KNOWN_RATING_SOURCES } from "../ratings/ratingSources";
import { asDate, asFlag, asList, asNumber, asText, prettyName, value } from "./editModel";
import { useSession } from "./formContext";
import { PROVIDER_ID_SOURCES, providerIdLabel } from "./providerIds";

/** A label above a control, with the reason a value was refused under it. */
function Labelled({
  label,
  error,
  children,
  id,
}: {
  label: string;
  error: string | undefined;
  children: ReactNode;
  id?: string;
}) {
  return (
    <div className="flex min-w-0 flex-col gap-1">
      <span id={id} className="type-body-strong text-text-2">
        {label}
      </span>
      {children}
      {error === undefined ? null : (
        <p role="alert" className="type-small m-0 text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

/** A single line of text. `suggestions` are offered, never enforced. */
export function TextControl({
  fieldKey,
  label,
  suggestions,
  placeholder,
  pretty = false,
}: {
  fieldKey: string;
  label: string;
  suggestions?: readonly string[];
  placeholder?: string;
  pretty?: boolean;
}) {
  const session = useSession();
  const listId = useId();
  const error = session.errors[fieldKey];
  const text = asText(session.get(fieldKey));
  return (
    <div className="min-w-0">
      <Field
        label={label}
        value={pretty ? prettyName(text) : text}
        placeholder={placeholder}
        error={error}
        list={suggestions === undefined ? undefined : listId}
        onChange={(event) => session.set(fieldKey, value.text(event.target.value))}
      />
      {suggestions === undefined ? null : (
        <datalist id={listId}>
          {suggestions.map((item) => (
            <option key={item} value={item} />
          ))}
        </datalist>
      )}
    </div>
  );
}

/** Several lines of text: the plot and the outline. */
export function TextAreaControl({
  fieldKey,
  label,
  rows = 5,
}: {
  fieldKey: string;
  label: string;
  rows?: number;
}) {
  const session = useSession();
  const id = useId();
  const error = session.errors[fieldKey];
  return (
    <div className="flex flex-col gap-1">
      <label htmlFor={id} className="type-body-strong text-text-2">
        {label}
      </label>
      <div
        data-field=""
        className="rounded-(--r-6) border border-solid border-line-edge bg-surface-card px-2.5 py-1.5"
      >
        <textarea
          id={id}
          rows={rows}
          value={asText(session.get(fieldKey))}
          aria-invalid={error === undefined ? undefined : true}
          onChange={(event) => session.set(fieldKey, value.text(event.target.value))}
          className="type-reading block min-h-16 w-full resize-y bg-transparent text-text outline-none"
        />
      </div>
      {error === undefined ? null : (
        <p role="alert" className="type-small m-0 text-danger">
          {error}
        </p>
      )}
    </div>
  );
}

/** A number: a year, a runtime, a rank. Empty clears it. */
export function NumberControl({
  fieldKey,
  label,
  min,
  max,
  step,
  unit,
}: {
  fieldKey: string;
  label: string;
  min?: number;
  max?: number;
  step?: number;
  unit?: string;
}) {
  const session = useSession();
  const n = asNumber(session.get(fieldKey));
  return (
    <Field
      label={label}
      type="number"
      inputMode="decimal"
      min={min}
      max={max}
      step={step}
      value={n === null ? "" : String(n)}
      error={session.errors[fieldKey]}
      trailing={
        unit === undefined ? undefined : <span className="type-small text-text-muted">{unit}</span>
      }
      onChange={(event) => {
        const raw = event.target.value;
        session.set(fieldKey, value.number(raw === "" ? null : Number(raw)));
      }}
    />
  );
}

/** Whether text is a real calendar day written year-month-day. */
function isIsoDate(text: string): boolean {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
  if (match === null) return false;
  const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
  const probe = new Date(Date.UTC(year, month - 1, day));
  return (
    probe.getUTCFullYear() === year &&
    probe.getUTCMonth() === month - 1 &&
    probe.getUTCDate() === day
  );
}

/**
 * A date, written year-month-day in a plain text field (no browser calendar glyph or locale mask).
 * What is typed is kept as typed; the value reaches the form once it is a real day, or empty.
 */
export function DateControl({ fieldKey, label }: { fieldKey: string; label: string }) {
  const session = useSession();
  const date = asDate(session.get(fieldKey));
  const [text, setText] = useState(date);
  const [synced, setSynced] = useState(date);
  // A value set from outside (a reset, a fresh load) replaces what was typed.
  if (date !== synced) {
    setSynced(date);
    setText(date);
  }
  const problem = text !== "" && !isIsoDate(text) ? t("edit.date.format") : undefined;
  return (
    <Field
      label={label}
      type="text"
      inputMode="numeric"
      autoComplete="off"
      spellCheck={false}
      placeholder="YYYY-MM-DD"
      icon={Calendar}
      value={text}
      error={problem ?? session.errors[fieldKey]}
      onChange={(event) => {
        const next = event.target.value;
        setText(next);
        if (next === "") session.set(fieldKey, value.date(null));
        else if (isIsoDate(next)) session.set(fieldKey, value.date(next));
      }}
    />
  );
}

/** One choice of a few, as a Select. */
export function ChoiceControl({
  fieldKey,
  label,
  options,
}: {
  fieldKey: string;
  label: string;
  options: readonly { value: string; label: string }[];
}) {
  const session = useSession();
  const id = useId();
  const current = asText(session.get(fieldKey));
  // A stored value the list does not know stays selectable, so it is not lost by looking at it.
  const known = options.some((option) => option.value === current);
  const all = known ? options : [{ value: current, label: current }, ...options];
  return (
    <Labelled label={label} error={session.errors[fieldKey]} id={id}>
      <Select
        aria-labelledby={id}
        options={all}
        value={current}
        onValueChange={(next) => session.set(fieldKey, value.text(next))}
        className="w-full"
      />
    </Labelled>
  );
}

/** A yes or no setting with its label beside it. */
export function FlagControl({ fieldKey, label }: { fieldKey: string; label: string }) {
  const session = useSession();
  const id = useId();
  return (
    <div className="flex h-(--h-field) items-center gap-3 self-end">
      <Switch
        aria-labelledby={id}
        checked={asFlag(session.get(fieldKey))}
        onCheckedChange={(on) => session.set(fieldKey, value.flag(on))}
      />
      <span id={id} className="type-body">
        {label}
      </span>
    </div>
  );
}

/** A list of words as removable pills with a field that adds one on Enter. */
export function ListControl({
  fieldKey,
  label,
  suggestions,
  display = (item: string) => item,
  normalize = (typed: string) => typed.trim(),
}: {
  fieldKey: string;
  label: string;
  suggestions?: readonly string[];
  display?: (item: string) => string;
  normalize?: (typed: string) => string;
}) {
  const session = useSession();
  const listId = useId();
  const [typed, setTyped] = useState("");
  const items = asList(session.get(fieldKey));
  const add = () => {
    const next = normalize(typed);
    setTyped("");
    if (next === "" || items.some((item) => item.toLowerCase() === next.toLowerCase())) return;
    session.set(fieldKey, value.list([...items, next]));
  };
  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "Enter") return;
    event.preventDefault();
    add();
  };
  return (
    <Labelled label={label} error={session.errors[fieldKey]}>
      {items.length === 0 ? null : (
        <ul className="m-0 flex list-none flex-wrap gap-1.5 p-0">
          {items.map((item) => (
            <li
              key={item}
              className="inline-flex h-6 items-center gap-1 rounded-(--r-pill) bg-surface-active pr-1 pl-2.5 text-(length:--fs-12)"
            >
              {display(item)}
              <button
                type="button"
                aria-label={t("edit.list.remove", { value: display(item) })}
                onClick={() => session.set(fieldKey, value.list(items.filter((i) => i !== item)))}
                className="cv-hoverable inline-flex size-4 items-center justify-center rounded-(--r-pill) bg-transparent text-text-muted hover:text-text"
              >
                <X size={12} aria-hidden="true" />
              </button>
            </li>
          ))}
        </ul>
      )}
      <Field
        aria-label={label}
        placeholder={t("edit.list.add")}
        value={typed}
        list={suggestions === undefined ? undefined : listId}
        onChange={(event) => setTyped(event.target.value)}
        onKeyDown={onKeyDown}
        onBlur={add}
      />
      {suggestions === undefined ? null : (
        <datalist id={listId}>
          {suggestions.map((item) => (
            <option key={item} value={item} />
          ))}
        </datalist>
      )}
    </Labelled>
  );
}

/** Moves one row of a list to another place. */
function moved<T>(rows: readonly T[], from: number, to: number): T[] {
  const next = [...rows];
  const [row] = next.splice(from, 1);
  if (row !== undefined) next.splice(to, 0, row);
  return next;
}

function RowButtons({
  index,
  count,
  noun,
  onMove,
  onRemove,
}: {
  index: number;
  count: number;
  noun: string;
  onMove: (to: number) => void;
  onRemove: () => void;
}) {
  return (
    <span className="flex shrink-0 gap-0.5">
      <IconButton
        icon={ArrowUp}
        size="sm"
        aria-label={t("edit.row.up", { noun })}
        disabled={index === 0}
        onClick={() => onMove(index - 1)}
      />
      <IconButton
        icon={ArrowDown}
        size="sm"
        aria-label={t("edit.row.down", { noun })}
        disabled={index === count - 1}
        onClick={() => onMove(index + 1)}
      />
      <IconButton
        icon={X}
        size="sm"
        aria-label={t("edit.row.remove", { noun })}
        onClick={onRemove}
      />
    </span>
  );
}

/** Provider IDs: a source and a value per row. */
export function IdsControl({ fieldKey, label }: { fieldKey: string; label: string }) {
  const session = useSession();
  const current = session.get(fieldKey);
  const pairs: readonly EditPairDto[] = current?.type === "pairs" ? current.pairs : [];
  const set = (next: EditPairDto[]) => session.set(fieldKey, { type: "pairs", pairs: next });
  const sources = (known: string) =>
    [...new Set([...PROVIDER_ID_SOURCES, known])].map((source) => ({
      value: source,
      label:
        source === known && !PROVIDER_ID_SOURCES.includes(source)
          ? source
          : providerIdLabel(source),
    }));
  return (
    <Labelled label={label} error={session.errors[fieldKey]}>
      <div className="flex flex-col gap-2">
        {pairs.length === 0 ? (
          <p className="type-small m-0 text-text-muted">{t("edit.ids.none")}</p>
        ) : null}
        {pairs.map((pair, index) => (
          <div key={`${pair.key}:${index}`} className="flex items-center gap-2">
            <Select
              aria-label={t("edit.ids.source", { row: index + 1 })}
              options={sources(pair.key)}
              value={pair.key}
              onValueChange={(key) => set(pairs.map((p, i) => (i === index ? { ...p, key } : p)))}
              className="w-44 shrink-0"
            />
            <Field
              aria-label={t("edit.ids.value", { row: index + 1 })}
              mono
              placeholder={t("edit.ids.valuePlaceholder")}
              value={pair.value}
              boxClassName="flex-1"
              className="min-w-0 flex-1"
              onChange={(event) =>
                set(pairs.map((p, i) => (i === index ? { ...p, value: event.target.value } : p)))
              }
            />
            <IconButton
              icon={X}
              size="sm"
              aria-label={t("edit.row.remove", { noun: t("edit.ids.one") })}
              onClick={() => set(pairs.filter((_, i) => i !== index))}
            />
          </div>
        ))}
        <Button
          size="sm"
          icon={Plus}
          className="self-start"
          onClick={() => {
            const unused = PROVIDER_ID_SOURCES.find((s) => !pairs.some((p) => p.key === s));
            set([...pairs, { key: unused ?? PROVIDER_ID_SOURCES[0] ?? "tmdb", value: "" }]);
          }}
        >
          {t("edit.ids.add")}
        </Button>
      </div>
    </Labelled>
  );
}

/** Ratings per source: the value on the source's own scale, the scale and the votes. */
export function RatingsControl({ fieldKey, label }: { fieldKey: string; label: string }) {
  const session = useSession();
  const listId = useId();
  const current = session.get(fieldKey);
  const rows: readonly EditRatingDto[] = current?.type === "ratings" ? current.ratings : [];
  const set = (next: EditRatingDto[]) => session.set(fieldKey, { type: "ratings", ratings: next });
  const patch = (index: number, change: Partial<EditRatingDto>) =>
    set(rows.map((row, i) => (i === index ? { ...row, ...change } : row)));
  return (
    <Labelled label={label} error={session.errors[fieldKey]}>
      <div className="flex flex-col gap-2">
        {rows.length === 0 ? (
          <p className="type-small m-0 text-text-muted">{t("edit.ratings.empty")}</p>
        ) : null}
        {rows.map((row, index) => (
          <div key={index} className="flex items-center gap-2">
            <Field
              aria-label={t("edit.ratings.source", { row: index + 1 })}
              list={listId}
              value={row.source}
              className="w-44 shrink-0"
              onChange={(event) =>
                patch(index, { source: event.target.value.trim().toLowerCase() })
              }
            />
            <Field
              aria-label={t("edit.ratings.value", { source: row.source || String(index + 1) })}
              type="number"
              step="any"
              min={0}
              value={row.value === null ? "" : String(row.value)}
              className="w-24"
              onChange={(event) =>
                patch(index, {
                  value: event.target.value === "" ? null : Number(event.target.value),
                })
              }
            />
            <span className="type-small shrink-0 text-text-muted">
              {t("edit.ratings.outOf", { max: row.max })}
            </span>
            <Field
              aria-label={t("edit.ratings.votes", { source: row.source || String(index + 1) })}
              type="number"
              min={0}
              value={String(row.votes)}
              className="w-28"
              trailing={
                <span className="type-small text-text-muted">{t("edit.ratings.votesUnit")}</span>
              }
              onChange={(event) => patch(index, { votes: Number(event.target.value) })}
            />
            <IconButton
              icon={X}
              size="sm"
              aria-label={t("edit.row.remove", { noun: t("edit.ratings.one") })}
              onClick={() => set(rows.filter((_, i) => i !== index))}
            />
          </div>
        ))}
        <datalist id={listId}>
          {KNOWN_RATING_SOURCES.map((source) => (
            <option key={source.id} value={source.id} />
          ))}
        </datalist>
        <Button
          size="sm"
          icon={Plus}
          className="self-start"
          onClick={() => set([...rows, { source: "", value: null, max: 10, votes: 0 }])}
        >
          {t("edit.ratings.add")}
        </Button>
      </div>
    </Labelled>
  );
}

const CREW_KINDS = [
  "director",
  "writer",
  "producer",
  "composer",
  "editor",
  "camera",
  "other",
] as const;

/** Cast or crew: a name and a character or job per row, in billing order. */
export function PeopleControl({
  fieldKey,
  label,
  crew = false,
}: {
  fieldKey: string;
  label: string;
  crew?: boolean;
}) {
  const session = useSession();
  const current = session.get(fieldKey);
  const people: readonly EditPersonDto[] = current?.type === "people" ? current.people : [];
  const set = (next: EditPersonDto[]) => session.set(fieldKey, { type: "people", people: next });
  const who = crew ? t("edit.people.crewWho") : t("edit.people.castWho");
  const kinds = CREW_KINDS.map((kind) => ({ value: kind, label: prettyName(kind.toUpperCase()) }));
  return (
    <Labelled label={label} error={session.errors[fieldKey]}>
      <div className="flex flex-col gap-2">
        {people.length === 0 ? (
          <p className="type-small m-0 text-text-muted">{t("edit.people.empty")}</p>
        ) : null}
        {people.map((person, index) => (
          <div key={index} className="flex items-center gap-2">
            {crew ? (
              <Select
                aria-label={t("edit.people.job", { who, row: index + 1 })}
                options={
                  kinds.some((k) => k.value === person.kind)
                    ? kinds
                    : [{ value: person.kind, label: person.kind }, ...kinds]
                }
                value={person.kind}
                onValueChange={(kind) =>
                  set(people.map((p, i) => (i === index ? { ...p, kind } : p)))
                }
                className="w-36 shrink-0"
              />
            ) : null}
            <Field
              aria-label={t("edit.people.name", { who, row: index + 1 })}
              placeholder={t("edit.people.namePlaceholder")}
              value={person.name}
              className="min-w-0 flex-1"
              onChange={(event) =>
                set(people.map((p, i) => (i === index ? { ...p, name: event.target.value } : p)))
              }
            />
            <Field
              aria-label={t("edit.people.role", { who, row: index + 1 })}
              placeholder={
                crew ? t("edit.people.titlePlaceholder") : t("edit.people.rolePlaceholder")
              }
              value={person.role}
              className="min-w-0 flex-1"
              onChange={(event) =>
                set(people.map((p, i) => (i === index ? { ...p, role: event.target.value } : p)))
              }
            />
            <RowButtons
              index={index}
              count={people.length}
              noun={who}
              onMove={(to) => set(moved(people, index, to))}
              onRemove={() => set(people.filter((_, i) => i !== index))}
            />
          </div>
        ))}
        <Button
          size="sm"
          icon={Plus}
          className="self-start"
          onClick={() =>
            set([...people, { kind: crew ? "director" : "actor", name: "", role: "" }])
          }
        >
          {crew ? t("edit.people.addCrew") : t("edit.people.addCast")}
        </Button>
      </div>
    </Labelled>
  );
}

/** A row of controls laid out in two columns, one on a narrow page. */
export function Row({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cx("grid grid-cols-1 gap-x-4 gap-y-3 @pane-md:grid-cols-2", className)}>
      {children}
    </div>
  );
}
