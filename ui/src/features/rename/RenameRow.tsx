import { ArrowRight, File, Folder, TriangleAlert } from "lucide-react";
import type { Ref } from "react";

import type { ChoiceDto, ConflictDto, PreviewRowDto } from "../../bindings";
import { Button } from "../../components/Button";
import { Checkbox } from "../../components/Checkbox";
import { Icon } from "../../components/Icon";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { fullPath, conflictLine, displayPath, leafOf, rootText } from "./renameText";
import { WordDiffText } from "./WordDiffText";

/** The columns of the Rename list: the checkbox, the title, the names and the files link. */
export const ROW_COLUMNS = "grid-cols-[34px_200px_minmax(0,1fr)_96px]";

function Pair({
  icon,
  before,
  after,
  mark,
}: {
  icon: typeof Folder;
  before: string;
  after: string;
  /** The warning glyph replaces the arrow on a blocked name. */
  mark?: "warn";
}) {
  return (
    <div className="grid grid-cols-[18px_minmax(0,1fr)_18px_minmax(0,1fr)] items-center gap-1.5 leading-[22px]">
      <Icon icon={icon} size={14} className="text-text-muted" />
      <WordDiffText before={before} after={after} side="before" />
      <Icon
        icon={mark === "warn" ? TriangleAlert : ArrowRight}
        size={mark === "warn" ? 16 : 14}
        className={mark === "warn" ? "text-warn" : "text-text-muted"}
      />
      <WordDiffText
        before={before}
        after={after}
        side="after"
        className={mark === "warn" ? "text-warn" : undefined}
      />
    </div>
  );
}

/** The files of a row, each as where it is now and where it goes (a file that is trashed says so). */
function FileList({ row }: { row: PreviewRowDto }) {
  return (
    <ul
      aria-label={t("renamePage.files.list", { title: row.title })}
      className="m-0 mt-1.5 mb-0.5 ml-6 flex list-none flex-col border-l border-solid border-line-section pl-3"
    >
      {row.files.map((file, index) => {
        const from = displayPath(row.root, file.from);
        const [first] = file.to;
        const to = first === undefined ? null : displayPath(row.root, first);
        return (
          <li
            key={`${file.from}:${index}`}
            className="grid grid-cols-[minmax(0,1fr)_18px_minmax(0,1fr)] items-center gap-1.5 text-(length:--fs-12) leading-[21px]"
          >
            <span title={from} className="truncate text-text-muted">
              {from}
            </span>
            <Icon icon={ArrowRight} size={14} className="text-text-muted" />
            {to === null ? (
              <span className="text-text-muted">{t("renamePage.files.trashed")}</span>
            ) : (
              <WordDiffText before={leafOf(from)} after={leafOf(to)} side="after" />
            )}
          </li>
        );
      })}
    </ul>
  );
}

interface RenameRowProps {
  row: PreviewRowDto;
  conflict: ConflictDto | undefined;
  /** The title is ticked for the rename. */
  included: boolean;
  open: boolean;
  /** The row is the one the keyboard is on. */
  current: boolean;
  /** "Read now" is reading the video details. */
  reading: boolean;
  /** A resolution the user chose for this title: skipping it. */
  skipped: boolean;
  /** `virtualizer.measureElement`, for a row whose height changes as it opens. */
  measure: Ref<HTMLLIElement>;
  index: number;
  top: number;
  onToggle: () => void;
  onOpen: () => void;
  onChoose: (choice: ChoiceDto) => void;
  onClearChoice: () => void;
  onOpenFolders: (paths: string[]) => void;
  onReadNow: () => void;
  onFocusRow: () => void;
}

/**
 * One title of the Rename list: a checkbox, the title with its year and library
 * folder, the folder name and the main file name as now → after pairs with word-level diffs, and
 * "Show all N files". A title held back for its video details has no checkbox and offers Read now;
 * a title whose new name is taken says so in the row, with the way out (spec `renamer` "Inline
 * rename conflicts"), and is not counted until it is resolved.
 */
export function RenameRow({
  row,
  conflict,
  included,
  open,
  current,
  reading,
  skipped,
  measure,
  index,
  top,
  onToggle,
  onOpen,
  onChoose,
  onClearChoice,
  onOpenFolders,
  onReadNow,
  onFocusRow,
}: RenameRowProps) {
  const held = row.status === "media_unread";
  const blocked = row.status === "conflict" && conflict !== undefined;
  const beforeFolder = displayPath(row.root, row.before);
  const afterFolder = displayPath(row.root, row.after);
  const folderChanges = row.before !== row.after;
  const fileChanges = row.before_file !== "" && row.before_file !== row.after_file;
  const meta =
    row.year > 0
      ? t("renamePage.row.meta", { year: String(row.year), folder: rootText(row.root) })
      : t("renamePage.row.metaNoYear", { folder: rootText(row.root) });
  // The first free numbered name the backend found for this title ("Crash (2004) (2)").
  const suggestion = conflict?.suggestions[conflict.items.indexOf(row.id)] ?? "";
  return (
    // The row is a list item; its controls (checkbox, buttons, links) are real elements.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions
    <li
      ref={measure}
      data-index={index}
      data-row={row.id}
      data-status={row.status}
      data-current={current ? "true" : undefined}
      onClick={onFocusRow}
      style={{ transform: `translateY(${top}px)` }}
      className={cx(
        "absolute top-0 left-0 grid w-full items-start border-b border-solid border-line-row px-6 py-[9px]",
        ROW_COLUMNS,
        blocked && "bg-warn-tint",
        open && !blocked && "bg-surface-card",
        (open || current) && "shadow-[inset_2px_0_0_var(--gold)]",
      )}
    >
      <span className="mt-1">
        {held ? null : (
          <Checkbox
            aria-label={t("renamePage.row.include", { title: row.title })}
            checked={included}
            disabled={blocked || skipped}
            onCheckedChange={onToggle}
          />
        )}
      </span>
      <div className="min-w-0 leading-[22px]">
        <div className="truncate font-(weight:--fw-600)">{row.title}</div>
        <div title={meta} className="cv-selectable truncate text-(length:--fs-12) text-text-muted">
          {meta}
        </div>
      </div>
      <div className="min-w-0 font-mono text-(length:--fs-12)">
        {held ? (
          <div
            title={row.note === "" ? t("renamePage.held") : row.note}
            className="grid grid-cols-[18px_minmax(0,1fr)] items-center gap-1.5 leading-[22px]"
          >
            <Icon icon={Folder} size={14} className="text-text-muted" />
            <span className="truncate text-text-muted">
              {beforeFolder} · <span className="font-sans">{t("renamePage.held")}</span>
            </span>
          </div>
        ) : (
          <>
            <Pair
              icon={Folder}
              before={beforeFolder}
              after={blocked ? afterFolder : folderChanges ? afterFolder : beforeFolder}
              {...(blocked ? { mark: "warn" as const } : {})}
            />
            {row.after_file === "" && !fileChanges ? null : (
              <Pair
                icon={File}
                before={row.before_file}
                after={fileChanges ? row.after_file : row.before_file}
              />
            )}
          </>
        )}
        {blocked && conflict !== undefined ? (
          <div className="mt-1 mb-0.5 ml-6 flex flex-wrap items-center gap-2 font-sans text-(length:--fs-13)">
            <span className="text-warn">{conflictLine(row, conflict)}</span>
            {conflict.options.includes("edit") && suggestion !== "" ? (
              <Button
                size="sm"
                onClick={() =>
                  onChoose({ id: row.id, kind: "edit", folder: suggestion, base: null })
                }
              >
                {t("renamePage.conflict.use", { name: suggestion })}
              </Button>
            ) : null}
            <Button
              size="sm"
              onClick={() =>
                onOpenFolders([fullPath(row.root, row.before), fullPath(row.root, conflict.path)])
              }
            >
              {t("renamePage.conflict.openBoth")}
            </Button>
            {conflict.options.includes("skip") ? (
              <Button
                size="sm"
                variant="ghost"
                onClick={() => onChoose({ id: row.id, kind: "skip", folder: null, base: null })}
              >
                {t("renamePage.conflict.skip")}
              </Button>
            ) : null}
          </div>
        ) : null}
        {skipped ? (
          <div className="mt-1 ml-6 flex items-center gap-2 font-sans text-(length:--fs-12) text-text-muted">
            {t("renamePage.skipped")}
            <button type="button" className="cv-link cursor-pointer" onClick={onClearChoice}>
              {t("renamePage.skipped.undo")}
            </button>
          </div>
        ) : null}
        <div className="cv-expand" data-open={open ? "true" : "false"}>
          <div>{open ? <FileList row={row} /> : null}</div>
        </div>
      </div>
      <div className="text-right text-(length:--fs-12) leading-[22px]">
        {held ? (
          <Button size="sm" disabled={reading} onClick={onReadNow}>
            {reading ? t("renamePage.held.reading") : t("renamePage.held.readNow")}
          </Button>
        ) : row.files.length > 0 ? (
          <button
            type="button"
            aria-expanded={open}
            className="cv-link cursor-pointer"
            onClick={onOpen}
          >
            {open
              ? t("renamePage.files.hide")
              : t("renamePage.files.show", { count: row.files.length })}
          </button>
        ) : null}
      </div>
    </li>
  );
}
