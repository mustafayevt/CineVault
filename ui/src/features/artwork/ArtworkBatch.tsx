import { useQueryClient } from "@tanstack/react-query";
import { Check, ChevronLeft, ChevronRight } from "lucide-react";
import { useCallback, useEffect, useState } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { IconButton } from "../../components/IconButton";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import {
  loadTitleSubject,
  subjectName,
  titleSubjectKey,
  useTitleSubject,
} from "../scrape/titleSubject";
import { usePickerClose } from "../scrape/usePickerClose";
import { useEscapeLayer } from "../shell/escStack";
import { ArtworkPicker } from "./ArtworkPicker";

/** How many title tiles the strip shows at once, around the title on screen. */
const STRIP_WINDOW = 12;

interface TitleTileProps {
  id: string;
  current: boolean;
  done: boolean;
  onGo: () => void;
}

/** One title of the strip: its poster, a gold ring when it is on screen and a green check once changed. */
function TitleTile({ id, current, done, onGo }: TitleTileProps) {
  const subject = useTitleSubject(id);
  const key = subject.data?.poster?.key ?? null;
  return (
    <button
      type="button"
      aria-label={t("artworkPicker.batch.title", {
        name: subject.data === undefined ? id : subjectName(subject.data),
      })}
      aria-current={current ? "true" : undefined}
      data-done={done ? "true" : undefined}
      onClick={onGo}
      className={cx(
        "cv-hoverable relative h-9 w-6 shrink-0 rounded-(--r-4) border-0 bg-surface-active p-0",
        current ? "outline-2 outline-offset-2 outline-gold" : "hover:brightness-125",
      )}
    >
      <span className="absolute inset-0 overflow-hidden rounded-[inherit]">
        {key !== null && isImageKey(key) ? (
          <CachedImage imageKey={key} size="list" alt="" className="size-full" />
        ) : subject.data === undefined ? null : (
          <PlaceholderPoster title={subject.data.title} className="size-full" />
        )}
      </span>
      {done ? (
        <span
          aria-hidden="true"
          className="absolute -right-1 -bottom-1 flex size-[13px] items-center justify-center rounded-(--r-pill) bg-ok text-on-gold"
        >
          <Icon icon={Check} size={12} className="size-2 stroke-[3.5]" />
        </span>
      ) : null}
    </button>
  );
}

interface ArtworkBatchProps {
  /** The titles to step through, in order. */
  ids: readonly string[];
  /** The type the picker opens on; it is kept as the picker steps to the next title. */
  type: string;
}

/**
 * Artwork for several titles (spec `artwork` "Artwork for several titles"): the
 * picker stepping through the selection one title at a time, with "Title 3 of 12", the title's
 * name, a strip of the titles, "2 changed · 0 skipped", "Use this and go to next" (Enter), "Use
 * this", "Skip title" and Done. Each choice is applied as it is made; the type stays chosen from
 * one title to the next. Nothing is picked for a skipped title.
 */
export function ArtworkBatch({ ids, type }: ArtworkBatchProps) {
  const queryClient = useQueryClient();
  const format = useFormat();
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const [index, setIndex] = useState(0);
  const [slot, setSlot] = useState(type);
  const [changed, setChanged] = useState<ReadonlySet<string>>(new Set());
  const [skipped, setSkipped] = useState<ReadonlySet<string>>(new Set());
  const last = ids.length - 1;
  const id = ids[index] ?? "";
  const subject = useTitleSubject(id);

  // The next title is read while this one is chosen for, so stepping shows it at once.
  useEffect(() => {
    const next = ids[index + 1];
    if (next === undefined) return;
    void queryClient.prefetchQuery({
      queryKey: titleSubjectKey(next),
      queryFn: () => loadTitleSubject(queryClient, next),
    });
  }, [ids, index, queryClient]);

  const go = useCallback((to: number) => setIndex(Math.min(last, Math.max(0, to))), [last]);
  /** Moves on, or finishes after the last title. */
  const advance = useCallback(() => {
    if (index >= last) close();
    else setIndex(index + 1);
  }, [close, index, last]);

  const windowStart = Math.max(0, Math.min(index - 5, ids.length - STRIP_WINDOW));
  const shown = ids.slice(windowStart, windowStart + STRIP_WINDOW);

  const header = (
    <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
      <span aria-hidden="true" className="absolute right-6 bottom-0 left-6 h-px bg-line-section" />
      <IconButton
        aria-label={t("artworkPicker.batch.previous")}
        icon={ChevronLeft}
        disabled={index === 0}
        onClick={() => go(index - 1)}
      />
      <div className="min-w-0">
        <div className="text-(length:--fs-12) text-text-muted">
          {t("artworkPicker.batch.step", { at: index + 1, count: ids.length })}
        </div>
        <h1 className="m-0 truncate text-(length:--fs-14) font-(weight:--fw-700)">
          {subject.data === undefined ? "…" : subjectName(subject.data)}
        </h1>
      </div>
      <IconButton
        aria-label={t("artworkPicker.batch.next")}
        icon={ChevronRight}
        disabled={index === last}
        onClick={() => go(index + 1)}
      />
      <div
        role="group"
        aria-label={t("artworkPicker.batch.titles")}
        className="flex flex-1 items-center justify-center gap-[5px]"
      >
        {shown.map((entry, position) => (
          <TitleTile
            key={entry}
            id={entry}
            current={windowStart + position === index}
            done={changed.has(entry)}
            onGo={() => go(windowStart + position)}
          />
        ))}
        {ids.length > shown.length ? (
          <span className="text-(length:--fs-12) text-text-muted">
            {t("artworkPicker.batch.more", { count: ids.length - shown.length })}
          </span>
        ) : null}
      </div>
      <span className="text-(length:--fs-12) text-text-muted" aria-live="polite">
        {t("artworkPicker.batch.counts", {
          changed: format.number(changed.size),
          skipped: format.number(skipped.size),
        })}
      </span>
      <Button variant="ghost" onClick={close}>
        {t("artworkPicker.done")}
      </Button>
    </div>
  );

  return (
    <section
      aria-label={t("artworkPicker.batch.region")}
      className="relative flex h-full min-h-0 flex-col"
    >
      {subject.data === undefined ? (
        <>
          {header}
          {subject.isError ? (
            <div className="flex flex-col items-start gap-3 px-6 pt-6">
              <p className="m-0 text-text-2">{t("artworkPicker.loadFailed")}</p>
              <Button onClick={advance}>{t("artworkPicker.skip")}</Button>
            </div>
          ) : (
            <div aria-busy="true" className="px-6 pt-4">
              <Skeleton shape="line" width="100%" height="34px" />
            </div>
          )}
        </>
      ) : (
        <ArtworkPicker
          // A new title starts a new picker; the type stays the one chosen.
          key={id}
          itemId={id}
          title={subjectName(subject.data)}
          initialSlot={slot}
          onSlotChange={setSlot}
          header={header}
          batch={{
            onNext: advance,
            onSkip: () => {
              if (!changed.has(id)) setSkipped((previous) => new Set([...previous, id]));
              advance();
            },
          }}
          // Each change is its own History entry (the backend records a placement at a time).
          onApplied={() => {
            setChanged((previous) => new Set([...previous, id]));
            setSkipped((previous) => new Set([...previous].filter((entry) => entry !== id)));
          }}
          onCancel={close}
        />
      )}
    </section>
  );
}
