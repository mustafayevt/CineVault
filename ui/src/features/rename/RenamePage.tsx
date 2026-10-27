import { useVirtualizer } from "@tanstack/react-virtual";
import { Link, useSearch } from "@tanstack/react-router";
import { PenLine } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";

import type { PreviewRowDto } from "../../bindings";
import { Button } from "../../components/Button";
import { Checkbox } from "../../components/Checkbox";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { useMediaInfoRead } from "../movie/useMediaInfoRead";
import { useOpenFolder } from "../detail/useOpenFolder";
import { RenameRow, ROW_COLUMNS } from "./RenameRow";
import { selectionOf } from "./renameSearch";
import type { RenameSearch } from "./renameSearch";
import { displayTemplate } from "./tokenLabels";
import { RENAME_KEY, useRenamePlan } from "./useRenamePlan";

type Family = "movies" | "tv";

/** The statuses the page lists: titles that would change, are blocked, are held back or were skipped. */
const LISTED: ReadonlySet<PreviewRowDto["status"]> = new Set([
  "change",
  "conflict",
  "media_unread",
  "skipped",
]);

const familyOf = (row: Pick<PreviewRowDto, "kind">): Family =>
  row.kind === "show" ? "tv" : "movies";

/** A naming template as the header shows it: the pattern in the mono face. */
function Pattern({ label, text }: { label: string; text: string }) {
  return (
    <>
      <span className="text-(length:--fs-12) text-text-muted">{label}</span>
      <span
        title={text}
        className="cv-selectable max-w-[22rem] truncate rounded-(--r-4) border border-solid border-line-section bg-surface-card px-[7px] py-0.5 font-mono text-(length:--fs-12) text-text"
      >
        {displayTemplate(text)}
      </span>
    </>
  );
}

function Empty({ selection }: { selection: boolean }) {
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 px-6 text-center">
      <h2 className="m-0 text-(length:--fs-16) font-(weight:--fw-700)">
        {t("renamePage.empty.title")}
      </h2>
      <p className="m-0 max-w-[48ch] text-text-2">
        {selection ? t("renamePage.empty.selection") : t("renamePage.empty.body")}
      </p>
      <Link to="/settings" search={{ section: "naming" }} className="cv-link">
        {t("renamePage.empty.edit")}
      </Link>
    </div>
  );
}

/**
 * The Rename page (`/rename`): every title whose folder or main file would change,
 * with a checkbox per title (all ticked at first), the folder and the main file as now → after
 * with word-level diffs, "Show all N files" in place, held-back titles with Read now, and inline
 * conflicts. The templates are shown in the header and edited only in Settings. Nothing touches the
 * disk until "Rename N titles"; the footer counts what is ticked and what is blocked.
 */
export function RenamePage() {
  const format = useFormat();
  const search = useSearch({ strict: false }) as RenameSearch;
  const scope = useMemo(() => selectionOf(search), [search]);
  const plan = useRenamePlan(scope);
  const read = useMediaInfoRead([RENAME_KEY]);
  const openFolder = useOpenFolder(t("renamePage.region"));
  const { preview } = plan;

  const [familyChoice, setFamilyChoice] = useState<Family | null>(null);
  const [excluded, setExcluded] = useState<ReadonlySet<string>>(new Set());
  const [open, setOpen] = useState<ReadonlySet<string>>(new Set());
  const [currentId, setCurrentId] = useState<string | null>(null);

  const listed = useMemo(
    () => (preview?.rows ?? []).filter((row) => LISTED.has(row.status)),
    [preview],
  );
  const counts = useMemo(() => {
    const movies = listed.filter((row) => familyOf(row) === "movies").length;
    return { movies, tv: listed.length - movies };
  }, [listed]);
  // The type that has titles to rename; movies when both or neither do.
  const family: Family = familyChoice ?? (counts.movies === 0 && counts.tv > 0 ? "tv" : "movies");
  const rows = useMemo(() => listed.filter((row) => familyOf(row) === family), [listed, family]);
  const conflictOf = useMemo(() => {
    const map = new Map<string, NonNullable<typeof preview>["conflicts"][number]>();
    for (const conflict of preview?.conflicts ?? []) {
      for (const id of conflict.items) map.set(id, conflict);
    }
    return map;
  }, [preview]);

  const clean = rows.filter((row) => row.status === "change");
  const included = clean.filter((row) => !excluded.has(row.id));
  const conflicts = rows.filter((row) => row.status === "conflict").length;
  const selectable = clean.length + conflicts;

  const scroller = useRef<HTMLDivElement>(null);
  // eslint-disable-next-line react-hooks/incompatible-library
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => 76,
    overscan: 8,
    getItemKey: (index) => rows[index]?.id ?? index,
  });

  const toggle = (id: string) =>
    setExcluded((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  const setAll = (on: boolean) => setExcluded(on ? new Set() : new Set(clean.map((row) => row.id)));
  const toggleOpen = (id: string) =>
    setOpen((previous) => {
      const next = new Set(previous);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });

  // Ctrl+A ticks every title of the list, unless a text field has the keys.
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.key.toLowerCase() !== "a") return;
      const target = event.target;
      if (
        target instanceof HTMLInputElement
          ? target.type !== "checkbox"
          : target instanceof HTMLTextAreaElement
      ) {
        return;
      }
      event.preventDefault();
      setExcluded(new Set());
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const everyIncluded = clean.length > 0 && included.length === clean.length;
  const patternsOf = plan.patterns;
  const problem = preview?.problems[0];

  const apply = () => {
    void plan.apply(included.map((row) => row.id));
  };

  return (
    <section aria-label={t("renamePage.region")} className="relative flex h-full min-h-0 flex-col">
      <h1 className="sr-only">{t("renamePage.region")}</h1>
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-6 bottom-0 left-6 h-px bg-line-section"
        />
        <SegmentedControl<Family>
          aria-label={t("renamePage.tabs")}
          value={family}
          onChange={setFamilyChoice}
          options={[
            {
              value: "movies",
              label: t("renamePage.tab.movies"),
              count: format.number(counts.movies),
            },
            { value: "tv", label: t("renamePage.tab.tv"), count: format.number(counts.tv) },
          ]}
        />
        <span className="grow" />
        {patternsOf === null ? null : (
          <>
            <Pattern
              label={t("renamePage.folder")}
              text={family === "movies" ? patternsOf.movie_folder : patternsOf.show_folder}
            />
            <span className="w-1.5" />
            <Pattern
              label={t("renamePage.file")}
              text={family === "movies" ? patternsOf.movie_file : patternsOf.episode_file}
            />
          </>
        )}
        <Link
          to="/settings"
          search={{ section: "naming" }}
          className="cv-link ml-1.5 text-(length:--fs-12)"
        >
          {t("renamePage.edit")}
        </Link>
      </div>

      {problem === undefined ? null : (
        <p role="alert" className="m-0 px-6 pt-3 text-warn">
          {t("renamePage.problem", { message: problem.message })}
        </p>
      )}

      {plan.error !== null && preview === null ? (
        <div role="alert" className="flex flex-col items-start gap-2 px-6 pt-4">
          <p className="m-0 text-danger">
            {t("renamePage.error", {
              message: plan.error instanceof Error ? plan.error.message : String(plan.error),
            })}
          </p>
          <Button onClick={plan.refetch}>{t("renamePage.retry")}</Button>
        </div>
      ) : plan.loading && preview === null ? (
        <div aria-busy="true" className="flex flex-col gap-2 px-6 pt-4">
          {[0, 1, 2].map((n) => (
            <Skeleton key={n} shape="line" width="100%" height="56px" />
          ))}
        </div>
      ) : listed.length === 0 ? (
        <Empty selection={scope !== null} />
      ) : (
        <>
          <div
            className={`grid h-[30px] shrink-0 items-center border-b border-solid border-line-section px-6 text-(length:--fs-12) font-(weight:--fw-600) text-text-muted ${ROW_COLUMNS}`}
          >
            <span>
              <Checkbox
                aria-label={t("renamePage.selectAll")}
                checked={everyIncluded}
                indeterminate={included.length > 0 && !everyIncluded}
                disabled={clean.length === 0}
                onCheckedChange={setAll}
              />
            </span>
            <span>{t("renamePage.col.title")}</span>
            <span>{t("renamePage.col.now")}</span>
            <span className="text-right">{t("renamePage.col.files")}</span>
          </div>
          <div ref={scroller} className="min-h-0 flex-1 overflow-auto">
            <ul
              aria-label={t("renamePage.list")}
              className="relative m-0 list-none p-0"
              style={{ height: virtualizer.getTotalSize() }}
            >
              {virtualizer.getVirtualItems().map((item) => {
                const row = rows[item.index];
                if (row === undefined) return null;
                return (
                  <RenameRow
                    key={row.id}
                    row={row}
                    conflict={conflictOf.get(row.id)}
                    included={row.status === "change" && !excluded.has(row.id)}
                    open={open.has(row.id)}
                    current={currentId === row.id}
                    reading={read.reading}
                    skipped={row.status === "skipped"}
                    measure={virtualizer.measureElement}
                    index={item.index}
                    top={item.start}
                    onToggle={() => toggle(row.id)}
                    onOpen={() => toggleOpen(row.id)}
                    onChoose={plan.choose}
                    onClearChoice={() => plan.clearChoice(row.id)}
                    onOpenFolders={(paths) => paths.forEach((path) => openFolder(path))}
                    onReadNow={read.readNow}
                    onFocusRow={() => setCurrentId(row.id)}
                  />
                );
              })}
            </ul>
          </div>
          {read.note === null ? null : <div className="shrink-0 px-6 py-2">{read.note}</div>}
          {/* The bar would ideally bleed under the floating panel; a page cannot paint outside its column,
              so it ends as a rounded card one gutter in from the panel instead of a square edge. */}
          <div className="mx-(--gutter) mb-(--gutter) flex h-14 shrink-0 items-center gap-3 rounded-(--r-10) border border-solid border-line-section bg-surface-card px-4">
            <span className="font-(weight:--fw-600)">
              {t("renamePage.selected", {
                selected: format.number(included.length),
                total: format.number(selectable),
              })}
            </span>
            {conflicts > 0 ? (
              <span className="text-(length:--fs-13) text-warn">
                {t("renamePage.conflicts", { count: conflicts })}
              </span>
            ) : null}
            <span className="grow" />
            <Button
              variant="primary"
              size="lg"
              icon={PenLine}
              title={t("renamePage.apply.tip")}
              disabled={plan.applying}
              {...(included.length === 0 ? { disabledReason: t("renamePage.apply.none") } : {})}
              onClick={apply}
            >
              {t("renamePage.apply", { count: included.length })}
            </Button>
          </div>
        </>
      )}
    </section>
  );
}
