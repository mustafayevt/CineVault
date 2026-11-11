import { useQueryClient } from "@tanstack/react-query";
import { CircleAlert, TriangleAlert } from "lucide-react";
import { useEffect, useRef } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { ProgressBar } from "../../components/ProgressBar";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { currentLibraryOpsBackend } from "../library-ops/libraryOpsBackend";
import { useMoveToStore } from "../move-to/moveToStore";
import { useNotificationStore } from "../notifications/notificationStore";
import type { AppNotification } from "../notifications/notificationStore";
import { fetchMissingRatings } from "../ratings/ratingSweep";
import { OPEN_RENAME_ACTION, openRename } from "../rename/runRename";
import { useNeedsAttention } from "./attentionModel";
import { openProviderKey } from "./openActivity";
import type { AttentionEntry } from "./attentionModel";
import type { UnfinishedBatch } from "./recoveryBackend";
import {
  batchLabel,
  batchProgress,
  resumeBatch,
  rollBackBatch,
  useRecoveryStore,
} from "./resolveUnfinished";

const rowClass =
  "flex flex-wrap items-start gap-x-3 gap-y-2 rounded-(--r-6) px-3 py-2.5 @max-[640px]/pane:px-2";

/** "Today 14:24" for today, else the date and time. */
function useWhen(): (ms: number | null) => string {
  const format = useFormat();
  return (ms) => {
    if (ms === null || ms <= 0) return "";
    const day = new Date(ms).toDateString();
    return day === new Date().toDateString()
      ? `${t("history.day.today")} ${format.time(ms)}`
      : format.dateTime(ms);
  };
}

function RowIcon({ tone }: { tone: "warn" | "danger" }) {
  return (
    <span
      aria-hidden="true"
      className={cx(
        "grid size-[30px] shrink-0 place-items-center rounded-(--r-6)",
        tone === "warn" ? "bg-warn-tint text-warn" : "bg-surface-active text-danger",
      )}
    >
      <Icon icon={tone === "warn" ? TriangleAlert : CircleAlert} size={16} />
    </span>
  );
}

/**
 * An unfinished move or rename: where it was going, its titles and how far it got, why it
 * stopped, and Resume and Roll back. While it is resumed, its progress with Pause and Cancel.
 */
function BatchRow({ batch, focused }: { batch: UnfinishedBatch; focused: boolean }) {
  const format = useFormat();
  const when = useWhen();
  const first = useRef<HTMLButtonElement>(null);
  const running = useMoveToStore((state) =>
    state.running?.batchId === batch.batch_id ? state.running : null,
  );
  const otherMove = useMoveToStore(
    (state) => state.running !== null && state.running.batchId !== batch.batch_id,
  );
  const work = useRecoveryStore((state) => state.working[batch.batch_id]);
  const resuming = work === "resume";

  // "Open in Activity" lands here: the row comes into view with the focus on its first button.
  useEffect(() => {
    if (!focused) return;
    first.current?.scrollIntoView({ block: "nearest" });
    first.current?.focus();
  }, [focused]);

  const destination = batch.destination;
  const title =
    batch.kind === "rename"
      ? t("activity.batch.rename")
      : resuming
        ? destination === null
          ? t("activity.batch.runningNoPlace")
          : t("activity.batch.running", { destination })
        : destination === null
          ? t("activity.batch.moveNoPlace")
          : t("activity.batch.move", { destination });
  const meta = [batchLabel(batch), batchProgress(batch, format.size), when(batch.created_ms)]
    .filter((part) => part !== "")
    .join(" · ");

  return (
    <li
      data-batch={batch.batch_id}
      className={cx(rowClass, focused && "bg-surface-active shadow-[inset_2px_0_0_var(--gold)]")}
    >
      <RowIcon tone="warn" />
      <div className="flex min-w-0 flex-1 basis-[280px] flex-col gap-0.5">
        <span className="font-(weight:--fw-600) text-text">{title}</span>
        <span className="text-(length:--fs-12) text-text-2 tabular-nums">{meta}</span>
        {resuming ? (
          <div className="mt-1.5 flex flex-col gap-1">
            <ProgressBar
              label={title}
              percent={running?.percent ?? 0}
              indeterminate={running === null}
              size="sm"
            />
            {running === null ? null : (
              <span className="truncate text-(length:--fs-12) text-text-muted tabular-nums">
                {[
                  `${Math.round(running.percent)}%`,
                  running.paused ? t("activity.batch.paused") : running.speed,
                  running.current,
                ]
                  .filter((part) => part !== undefined && part !== "")
                  .join(" · ")}
              </span>
            )}
          </div>
        ) : (
          <span className="text-(length:--fs-12) text-text-muted">{batch.reason}</span>
        )}
      </div>
      <div className="ml-auto flex shrink-0 items-center gap-1.5">
        {resuming ? (
          running === null ? null : (
            <>
              <Button
                ref={first}
                size="sm"
                onClick={() => {
                  const paused = !running.paused;
                  useMoveToStore.getState().setRunning({ ...running, paused });
                  void currentLibraryOpsBackend().controlMove(paused ? "pause" : "resume");
                }}
              >
                {running.paused ? t("activity.batch.continue") : t("activity.batch.pause")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                title={t("activity.batch.cancelHint")}
                onClick={() => void currentLibraryOpsBackend().controlMove("cancel")}
              >
                {t("activity.batch.cancel")}
              </Button>
            </>
          )
        ) : (
          <>
            <Button
              ref={first}
              size="sm"
              variant="primary"
              disabled={work !== undefined || otherMove}
              title={otherMove ? t("activity.batch.busy") : t("activity.batch.resumeHint")}
              onClick={() => void resumeBatch(batch)}
            >
              {t("activity.batch.resume")}
            </Button>
            <Button
              size="sm"
              disabled={work !== undefined}
              title={t("activity.batch.rollBackHint")}
              onClick={() => void rollBackBatch(batch)}
            >
              {t("activity.batch.rollBack")}
            </Button>
          </>
        )}
      </div>
    </li>
  );
}

/** A warning or an error from background work, with its fix when it has one, and Dismiss. */
function NoticeRow({ notice }: { notice: AppNotification }) {
  const when = useWhen();
  const queryClient = useQueryClient();
  const dismiss = useNotificationStore((state) => state.dismiss);
  const meta = [notice.source ?? "", when(notice.at)].filter((part) => part !== "").join(" · ");
  return (
    <li data-notice={notice.id} data-severity={notice.severity} className={rowClass}>
      <RowIcon tone={notice.severity === "error" ? "danger" : "warn"} />
      <div className="flex min-w-0 flex-1 basis-[280px] flex-col gap-0.5">
        <span className="font-(weight:--fw-600) text-text">
          {notice.title}
          {notice.repeats === undefined ? null : (
            <span className="ml-1.5 text-(length:--fs-12) font-(weight:--fw-400) text-text-muted tabular-nums">
              {t("activity.notice.repeats", { count: notice.repeats })}
            </span>
          )}
        </span>
        {notice.detail === undefined || notice.detail === "" ? null : (
          <span className="cv-selectable text-(length:--fs-12) break-words whitespace-pre-line text-text-2">
            {notice.detail}
          </span>
        )}
        {meta === "" ? null : (
          <span className="text-(length:--fs-12) text-text-muted tabular-nums">{meta}</span>
        )}
      </div>
      <div className="ml-auto flex shrink-0 items-center gap-1.5">
        {notice.providerKey === undefined ? null : (
          <Button size="sm" onClick={() => openProviderKey(notice.providerKey ?? "")}>
            {t("activity.notice.openKey")}
          </Button>
        )}
        {notice.action === "retry_imdb_dataset" ? (
          <Button
            size="sm"
            onClick={() => {
              dismiss(notice.id);
              void fetchMissingRatings(queryClient);
            }}
          >
            {t("activity.notice.retry")}
          </Button>
        ) : null}
        {notice.action === OPEN_RENAME_ACTION && (notice.itemIds?.length ?? 0) > 0 ? (
          <Button
            size="sm"
            onClick={() => {
              // The preview is the retry: it shows the plan as the disk is now.
              dismiss(notice.id);
              openRename(notice.itemIds ?? []);
            }}
          >
            {t("renamePage.notice.open")}
          </Button>
        ) : null}
        <Button
          size="sm"
          variant="ghost"
          aria-label={t("activity.notice.dismissLabel", { title: notice.title })}
          onClick={() => dismiss(notice.id)}
        >
          {t("activity.notice.dismiss")}
        </Button>
      </div>
    </li>
  );
}

function EntryRow({ entry, focusBatch }: { entry: AttentionEntry; focusBatch: string | null }) {
  return entry.kind === "batch" ? (
    <BatchRow batch={entry.batch} focused={focusBatch === entry.batch.batch_id} />
  ) : (
    <NoticeRow notice={entry.notice} />
  );
}

interface NeedsAttentionProps {
  /** The unfinished batch to show first and focus, from "Open in Activity". */
  focusBatch?: string | null;
}

/**
 * The top of Activity (spec `drives` "Unfinished moves are recovered"): what needs the user,
 * shown only while something does. Unfinished moves and renames with Resume and Roll back (and
 * live progress while one is resumed), then warnings and errors from background work with their
 * fix and Dismiss. It scrolls on its own when it is long, so History stays in view below it.
 */
export function NeedsAttention({ focusBatch = null }: NeedsAttentionProps) {
  const entries = useNeedsAttention();
  const notices = entries.filter((entry) => entry.kind === "notice");
  if (entries.length === 0) return null;
  const dismissAll = () => {
    const store = useNotificationStore.getState();
    for (const entry of notices) store.dismiss(entry.notice.id);
  };
  return (
    <section
      aria-labelledby="needs-attention-heading"
      data-testid="needs-attention"
      className="flex max-h-[45%] shrink-0 flex-col border-b border-solid border-line-section px-6 pt-3 pb-3 @max-[640px]/pane:px-4"
    >
      <div className="flex h-8 shrink-0 items-center gap-2">
        <h2
          id="needs-attention-heading"
          className="m-0 text-(length:--fs-13) font-(weight:--fw-600) text-text"
        >
          {t("activity.attention")}
        </h2>
        <span className="rounded-full bg-warn-tint px-2 text-(length:--fs-12) leading-5 text-warn tabular-nums">
          {t("activity.attention.count", { count: entries.length })}
        </span>
        <span className="flex-1" />
        {notices.length === 0 ? null : (
          <Button
            size="sm"
            variant="ghost"
            title={t("activity.attention.dismissAllHint")}
            onClick={dismissAll}
          >
            {t("activity.attention.dismissAll")}
          </Button>
        )}
      </div>
      <ul className="m-0 mt-1.5 flex min-h-0 list-none flex-col gap-0.5 overflow-y-auto rounded-(--r-10) border border-solid border-line-edge bg-surface-card p-1">
        {entries.map((entry) => (
          <EntryRow key={entry.key} entry={entry} focusBatch={focusBatch} />
        ))}
      </ul>
    </section>
  );
}
