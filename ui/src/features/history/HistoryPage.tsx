import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  ArrowLeftRight,
  ArrowRight,
  ChevronDown,
  ChevronRight,
  Image,
  PenLine,
  ScanSearch,
  Trash2,
  Undo2,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { Button } from "../../components/Button";
import { SectionLabel } from "../../components/SectionLabel";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { Tag } from "../../components/Tag";
import { Icon } from "../../components/Icon";
import { cx } from "../../lib/cx";
import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { currentHistoryBackend, HISTORY_KEY } from "./historyBackend";
import type { HistoryEntry, HistoryFilter, HistoryTitle } from "./historyBackend";
import { groupByDay, PAGE_SIZE } from "./historyModel";
import type { Day } from "./historyModel";
import { undoHistoryEntry } from "./historyUndo";

const TABS: readonly { value: HistoryFilter; label: MessageKey }[] = [
  { value: "all", label: "history.tab.all" },
  { value: "renames", label: "history.tab.renames" },
  { value: "moves", label: "history.tab.moves" },
  { value: "deletes", label: "history.tab.deletes" },
  { value: "artwork_and_matches", label: "history.tab.artwork" },
];

/** The neutral icon of a kind of entry; only a permanent delete is drawn in the danger colour. */
const KIND_ICON: Record<string, LucideIcon> = {
  rename: PenLine,
  move: ArrowLeftRight,
  delete: Trash2,
  delete_permanent: Trash2,
  artwork: Image,
  match: ScanSearch,
};

function TitleLine({ title }: { title: HistoryTitle }) {
  const undone = title.state === "undone";
  return (
    <li
      className={cx(
        "grid grid-cols-[minmax(0,1fr)_18px_minmax(0,1fr)] items-center gap-2 font-mono text-(length:--fs-12) leading-6",
        undone && "text-text-disabled line-through",
      )}
    >
      {title.after === "" ? (
        <span className="col-span-3 cv-selectable truncate text-text-muted" title={title.before}>
          {title.before === "" ? title.title : title.before}
        </span>
      ) : (
        <>
          <span className="cv-selectable truncate text-text-muted" title={title.before}>
            {title.before === "" ? title.title : title.before}
          </span>
          <Icon icon={ArrowRight} size={12} className="text-text-muted" />
          <span className="cv-selectable truncate" title={title.after}>
            {title.after}
          </span>
        </>
      )}
    </li>
  );
}

/** The first titles of an entry and a "Show N more" link that lists the rest in place. */
function Titles({ entry }: { entry: HistoryEntry }) {
  const [all, setAll] = useState(false);
  const full = useQuery({
    queryKey: [...HISTORY_KEY, "titles", entry.id, entry.titles_undone],
    queryFn: () => currentHistoryBackend().titles(entry.id),
    enabled: all,
    staleTime: 30_000,
  });
  const shown = all && full.data !== undefined ? full.data : entry.preview;
  const more = entry.titles_total - entry.preview.length;
  return (
    <div className="px-3 pt-0 pb-3 pl-14">
      <ul className="m-0 flex list-none flex-col p-0">
        {shown.map((title) => (
          <TitleLine key={title.id} title={title} />
        ))}
      </ul>
      {more > 0 && (
        <button
          type="button"
          className="mt-1 cursor-pointer border-0 bg-transparent p-0 text-(length:--fs-12) leading-[26px] text-text-2 hover:text-text hover:underline"
          onClick={() => setAll((open) => !open)}
        >
          {all ? t("history.showLess") : t("history.showMore", { count: more })}
        </button>
      )}
    </div>
  );
}

function ActionCell({
  entry,
  busy,
  onUndo,
}: {
  entry: HistoryEntry;
  busy: boolean;
  onUndo: () => void;
}) {
  if (entry.undo_state === "none") {
    // An entry whose files were erased or emptied from the trash says why its Undo is off.
    const note = entry.undo_note ?? null;
    return (
      <span className="flex min-w-0 flex-col items-end text-right text-(length:--fs-12) text-text-muted">
        {t("history.noUndo")}
        {note === null ? null : (
          <span data-undo-note="" className="text-(length:--fs-11) leading-tight" title={note}>
            {note}
          </span>
        )}
      </span>
    );
  }
  if (entry.undo_state === "undone") {
    return (
      <span className="justify-self-end">
        <Tag>{t("history.undone")}</Tag>
      </span>
    );
  }
  const partial = entry.undo_state === "partial";
  const text = partial
    ? t("history.undoRest")
    : entry.kind === "delete"
      ? t("history.restore")
      : entry.titles_total > 1
        ? t("history.undoAll")
        : t("history.undo");
  return (
    <span className="flex flex-col items-end gap-0.5">
      <Button
        onClick={onUndo}
        disabled={busy}
        icon={!partial && entry.kind !== "delete" && entry.titles_total > 1 ? Undo2 : undefined}
        aria-label={`${text}: ${entry.summary}`}
      >
        {text}
      </Button>
      {partial && (
        <span className="text-(length:--fs-11) text-text-muted">
          {t("history.partial", { done: entry.titles_undone, total: entry.titles_total })}
        </span>
      )}
    </span>
  );
}

function EntryRow({ entry }: { entry: HistoryEntry }) {
  const format = useFormat();
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const expandable = entry.titles_total > 1 && entry.preview.length > 0;
  const undone = entry.undo_state === "undone";
  const permanent = entry.kind === "delete_permanent";
  const icon = KIND_ICON[entry.kind] ?? PenLine;
  const detail =
    undone && entry.undone_at !== null
      ? t("history.undoneAt", { time: format.time(entry.undone_at) })
      : entry.detail;

  const undo = () => {
    setBusy(true);
    undoHistoryEntry(entry.id)
      .then((report) => {
        if (report.left.length === 0 && report.undone.length > 0) {
          showToast({
            message: t(
              entry.kind === "delete" ? "history.toast.restored" : "history.toast.undone",
              {
                summary: entry.summary,
              },
            ),
          });
        }
      })
      .catch((error: unknown) =>
        notify({
          severity: "error",
          title: t("history.toast.failed", { summary: entry.summary }),
          detail: error instanceof Error ? error.message : String(error),
        }),
      )
      .finally(() => setBusy(false));
  };

  const summary = (
    <>
      <div className="font-(weight:--fw-600)">{entry.summary}</div>
      <div
        className={cx(
          "truncate text-(length:--fs-12) text-text-muted",
          entry.kind === "delete" && "cv-selectable font-mono",
        )}
        title={detail}
      >
        {detail}
      </div>
    </>
  );

  return (
    <div
      data-kind={entry.kind}
      data-state={entry.undo_state}
      className={cx(
        "rounded-(--r-6)",
        open && "bg-surface-active shadow-[inset_2px_0_0_var(--gold)]",
        undone && "text-text-disabled",
      )}
    >
      <div className="grid min-h-[52px] grid-cols-[32px_minmax(0,1fr)_120px_120px] items-center gap-3 px-3">
        <span
          className={cx(
            "grid size-[30px] place-items-center rounded-(--r-6) bg-surface-active text-text-2",
            permanent && "text-danger",
          )}
        >
          <Icon icon={icon} size={16} />
        </span>
        {expandable ? (
          <button
            type="button"
            aria-expanded={open}
            aria-label={t(open ? "history.collapse" : "history.expand", { summary: entry.summary })}
            onClick={() => setOpen((value) => !value)}
            className="flex min-w-0 cursor-pointer items-center gap-1.5 border-0 bg-transparent p-0 text-left text-inherit"
          >
            <span className="min-w-0 flex-1">{summary}</span>
            <Icon
              icon={open ? ChevronDown : ChevronRight}
              size={14}
              className="shrink-0 text-text-muted"
            />
          </button>
        ) : (
          <div className="min-w-0">{summary}</div>
        )}
        <span className="text-right text-text-2">
          {entry.at === null ? "" : format.time(entry.at)}
        </span>
        <span className="justify-self-end">
          <ActionCell entry={entry} busy={busy} onUndo={undo} />
        </span>
      </div>
      {open && expandable && <Titles entry={entry} />}
    </div>
  );
}

function DaySection({ day }: { day: Day }) {
  return (
    <section aria-label={day.label}>
      <SectionLabel className="mt-5 mb-2 px-4 first:mt-3.5">{day.label}</SectionLabel>
      <div className="flex flex-col gap-0.5 rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-1">
        {day.entries.map((entry) => (
          <EntryRow key={entry.id} entry={entry} />
        ))}
      </div>
    </section>
  );
}

/**
 * The History page (`/history`): every rename, move, delete, artwork change and
 * change of match, newest first and grouped by day, with the tabs All · Renames · Moves · Deletes
 * · Artwork and matches and how much is still in the Recycle Bin. Each entry offers one action:
 * Undo, Undo all, Restore, Undone or No undo. An entry of several titles expands to their old and
 * new names. The page follows `history-changed`, so a change made anywhere shows at once.
 */
export function HistoryPage() {
  const format = useFormat();
  const client = useQueryClient();
  const [filter, setFilter] = useState<HistoryFilter>("all");

  const list = useInfiniteQuery({
    queryKey: [...HISTORY_KEY, "list", filter],
    queryFn: ({ pageParam }) => currentHistoryBackend().list(filter, pageParam, PAGE_SIZE),
    initialPageParam: 0,
    getNextPageParam: (last, pages) => {
      const loaded = pages.reduce((sum, page) => sum + page.entries.length, 0);
      return loaded < (last.total ?? 0) && last.entries.length > 0 ? loaded : undefined;
    },
    staleTime: 0,
  });
  const bin = useQuery({
    queryKey: [...HISTORY_KEY, "bin"],
    queryFn: () => currentHistoryBackend().binTotal(),
    staleTime: 0,
  });

  useEffect(
    () =>
      currentHistoryBackend().onChanged(
        () => void client.invalidateQueries({ queryKey: HISTORY_KEY }),
      ),
    [client],
  );

  const entries = useMemo(
    () => list.data?.pages.flatMap((page) => page.entries) ?? [],
    [list.data],
  );
  const days = useMemo(
    () => groupByDay(entries, new Date(), (ms) => format.date(ms)),
    [entries, format],
  );

  return (
    <section aria-label={t("history.region")} className="flex h-full min-h-0 flex-col">
      <h1 className="sr-only">{t("history.region")}</h1>
      <div className="px-6">
        <div className="flex h-12 items-center gap-2.5 border-b border-solid border-line-section">
          <SegmentedControl<HistoryFilter>
            aria-label={t("history.tabs")}
            value={filter}
            onChange={setFilter}
            options={TABS.map((tab) => ({ value: tab.value, label: t(tab.label) }))}
          />
          <span className="flex-1" />
          {bin.data !== null && bin.data !== undefined && (
            <span
              className="text-(length:--fs-12) text-text-muted"
              title={t("history.bin.tip")}
              data-testid="bin-total"
            >
              {t("history.bin", { size: format.size(bin.data) })}
            </span>
          )}
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto px-6 pb-4">
        {list.isPending ? (
          <div
            aria-busy="true"
            aria-label={t("history.loading")}
            className="flex flex-col gap-2 pt-5"
          >
            {[0, 1, 2, 3].map((n) => (
              <Skeleton key={n} shape="line" width="100%" height="52px" />
            ))}
          </div>
        ) : list.isError && entries.length === 0 ? (
          <div role="alert" className="flex flex-col items-start gap-2 pt-5">
            <p className="m-0 text-danger">{t("history.error")}</p>
            <Button onClick={() => void list.refetch()}>{t("history.retry")}</Button>
          </div>
        ) : entries.length === 0 ? (
          <div className="flex flex-col items-start gap-1 pt-6">
            <p className="m-0 text-(length:--fs-14) font-(weight:--fw-600)">
              {t("history.empty.title")}
            </p>
            <p className="m-0 max-w-[56ch] text-text-2">
              {t(filter === "all" ? "history.empty.body" : "history.empty.tab")}
            </p>
          </div>
        ) : (
          <>
            {days.map((day) => (
              <DaySection key={day.key} day={day} />
            ))}
            {list.hasNextPage && (
              <div className="pt-4">
                <Button
                  onClick={() => void list.fetchNextPage()}
                  disabled={list.isFetchingNextPage}
                >
                  {t("history.older")}
                </Button>
              </div>
            )}
          </>
        )}
      </div>
    </section>
  );
}
