import { useQuery } from "@tanstack/react-query";
import { Link, useNavigate, useParams } from "@tanstack/react-router";
import { useState } from "react";
import { AlertTriangle, Award, Check, Copy, HardDrive, Network } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { PlaceholderPoster } from "../../components/PlaceholderPoster";
import { Skeleton } from "../../components/Skeleton";
import { Tooltip } from "../../components/Tooltip";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { useDeleteStore } from "../delete/deleteActions";
import { runtimeLabel } from "../detail/detailFormat";
import { CachedImage } from "../images/CachedImage";
import { isImageKey } from "../images/imageUrl";
import { useSetting } from "../settings/settingsStore";
import { useEscapeLayer } from "../shell/escStack";
import { binName } from "../shell/binName";
import { detectPlatform } from "../shell/platform";
import { usePickerClose } from "../scrape/usePickerClose";
import { useTitleSubject } from "../scrape/titleSubject";
import { askAgain, copyName, keepBoth, keepThisOne } from "./copiesActions";
import { currentCopiesBackend } from "./copiesBackend";
import type { Comparison } from "./copiesBackend";
import { ROW_IDS, cardsOf, freedBytes, totalBytes } from "./compareModel";
import type { Card, Cell, RowId } from "./compareModel";
import { comparisonKey } from "./useCopiesOf";

const ROW_LABEL = {
  resolution: ["copies.row.resolution", "copies.row.resolution.hint"],
  hdr: ["copies.row.hdr", "copies.row.hdr.hint"],
  audio: ["copies.row.audio", "copies.row.audio.hint"],
  source: ["copies.row.source", "copies.row.source.hint"],
  bitrate: ["copies.row.bitrate", "copies.row.bitrate.hint"],
  length: ["copies.row.length", "copies.row.length.hint"],
  size: ["copies.row.size", "copies.row.size.hint"],
  folder: ["copies.row.folder", "copies.row.folder.hint"],
} as const satisfies Record<RowId, readonly [string, string]>;

/** Rows are 52px tall; the card head is 104px, so the label column starts under it. */
const ROW_HEIGHT = "h-[52px]";

/** The label column: each row's name with a plain hint under it. */
function Labels() {
  return (
    <div aria-hidden="true" className="w-[188px] shrink-0 pt-[106px]">
      {ROW_IDS.map((id) => (
        <div key={id} className={cx("flex flex-col justify-center gap-px", ROW_HEIGHT)}>
          <span className="font-(weight:--fw-500) text-text">{t(ROW_LABEL[id][0])}</span>
          <span className="text-(length:--fs-12) text-text-muted">{t(ROW_LABEL[id][1])}</span>
        </div>
      ))}
    </div>
  );
}

/** One value: the better one of a row is tinted, bold and checked; the others quieter. */
function Value({ cell }: { cell: Cell }) {
  const text = (
    <span className={cx("min-w-0", cell.mono === true ? "flex-1" : "truncate")}>
      {cell.mono === true ? (
        <PathText path={cell.text} tooltip={false} className="font-mono text-(length:--fs-12)" />
      ) : (
        cell.text
      )}
    </span>
  );
  const body = (
    <div
      data-row={cell.row}
      data-verdict={cell.verdict}
      className={cx(
        "flex h-9 min-w-0 flex-1 items-center gap-2 rounded-(--r-6) px-2",
        cell.verdict === "win" && "bg-selected-row font-(weight:--fw-600) text-text",
        cell.verdict === "lose" && "text-text-2",
        cell.verdict === "same" && "text-text-muted",
        cell.verdict === "plain" && (cell.mono === true ? "text-text-2" : "text-text"),
      )}
    >
      {text}
      {cell.verdict === "win" && (
        <Check
          size={14}
          strokeWidth={1.75}
          role="img"
          aria-label={t("copies.better")}
          className="shrink-0 text-gold"
        />
      )}
      {cell.note !== undefined && (
        <span className="ml-auto text-(length:--fs-12) font-(weight:--fw-500) whitespace-nowrap text-text-muted">
          {cell.note}
        </span>
      )}
    </div>
  );
  return cell.full !== undefined ? <Tooltip content={cell.full}>{body}</Tooltip> : body;
}

/** One copy: where it lives, its quality, every value, and Keep this one. */
function CopyCard({
  card,
  index,
  count,
  onKeep,
  busy,
}: {
  card: Card;
  index: number;
  count: number;
  /** A Keep this one is running: every card waits for it. */
  busy: boolean;
  /** `null` when the page offers no Keep this one (the copies may be one file). */
  onKeep: (() => void) | null;
}) {
  const { copy } = card;
  return (
    <article
      aria-label={t("copies.card.aria", {
        index: index + 1,
        count,
        title: card.heading,
        drive: card.drive,
      })}
      data-best={copy.best ? "true" : undefined}
      className={cx(
        "flex min-w-[260px] max-w-[400px] flex-1 flex-col overflow-hidden rounded-(--r-10)",
        "border border-solid bg-surface-card",
        copy.best ? "border-gold-tint-edge" : "border-line-section",
      )}
    >
      <div className="flex h-[104px] shrink-0 flex-col gap-1.5 border-b border-solid border-line-section px-4 pt-3.5">
        <div className="flex h-[22px] items-center gap-2">
          <span className="flex min-w-0 items-center gap-1.5 text-(length:--fs-12) text-text-2">
            <Icon icon={card.network ? Network : HardDrive} size={14} />
            <span className="sr-only">
              {card.network ? t("copies.card.network") : t("copies.card.local")}
            </span>
            <span className="truncate">{card.drive}</span>
          </span>
          <span className="grow" />
          {copy.best && (
            <span className="inline-flex h-[22px] items-center gap-[5px] rounded-(--r-pill) bg-gold-tint pr-[9px] pl-[7px] text-(length:--fs-12) font-(weight:--fw-600) whitespace-nowrap text-gold">
              <Icon icon={Award} size={14} />
              {t("copies.best")}
            </span>
          )}
        </div>
        <div className="truncate text-(length:--fs-16) font-(weight:--fw-700)">{card.heading}</div>
        <div
          title={copy.file_name}
          className="cv-selectable truncate font-mono text-(length:--fs-12) text-text-muted"
        >
          {copy.file_name}
        </div>
      </div>

      {card.cells.map((cell, rowIndex) => (
        <div
          key={cell.row}
          className={cx(
            "flex items-center px-2",
            ROW_HEIGHT,
            rowIndex > 0 && "border-t border-solid border-line-row",
          )}
        >
          <Value cell={cell} />
        </div>
      ))}

      {onKeep !== null && (
        <div className="mt-auto flex flex-col gap-2 border-t border-solid border-line-section px-4 py-3.5">
          <Button
            variant={copy.best ? "primary" : "default"}
            size="lg"
            className="w-full"
            aria-label={t("copies.keep.aria", { title: copyName(copy), summary: card.heading })}
            disabled={busy}
            onClick={onKeep}
          >
            {t("copies.keep")}
          </Button>
        </div>
      )}
    </article>
  );
}

/** Where deleted copies go now, in one sentence (the page links to the setting). */
function useDestination(count: number): string {
  const mode = useSetting<string>("files.deleted-mode");
  const folder = useSetting<string>("files.trash-folder");
  if (mode === "erase") return t("copies.dest.erase", { count });
  if (mode === "folder") return t("copies.dest.folder", { count });
  if (mode === "custom" && folder !== "") return t("copies.dest.custom", { count, folder });
  return t("copies.dest.bin", { count, bin: binName(detectPlatform()) });
}

function Page({ page, movieId }: { page: Comparison; movieId: string }) {
  const navigate = useNavigate();
  const format = useFormat();
  const close = usePickerClose();
  const subject = useTitleSubject(movieId);
  const destination = useDestination(page.copies.length - 1);

  const cards = cardsOf(page, {
    size: (bytes) => format.size(bytes),
    runtime: runtimeLabel,
    folder: (path) => path,
  });
  const own = page.copies[0];
  // The film's name as its copies carry it; the poster comes from the movie's own page.
  const name = own !== undefined ? copyName(own) : "";
  const poster = subject.data?.poster ?? null;
  const count = page.copies.length;

  // One Keep this one at a time: from the click until its confirmation is answered and its
  // delete has run, every card waits.
  const [checking, setChecking] = useState(false);
  const deleting = useDeleteStore(
    (state) =>
      state.confirm !== null ||
      state.request !== null ||
      page.copies.some((copy) => state.hidden.has(copy.id)),
  );
  const busy = checking || deleting;

  const keep = async (keepId: string) => {
    if (busy) return;
    setChecking(true);
    await keepThisOne(page, keepId, {
      // The opened copy is kept: back to where the page was opened from. The opened copy is the
      // one that goes: its own page is gone, so the kept copy's page opens in its place.
      done: () => {
        if (keepId === movieId) close();
        else void navigate({ to: "/movies/$movieId", params: { movieId: keepId }, replace: true });
      },
      rename: () => void navigate({ to: "/rename", search: { ids: keepId } as never }),
    }).finally(() => setChecking(false));
  };

  const both = async () => {
    if (await keepBoth(page)) close();
  };

  return (
    <section
      aria-label={t("copies.page.region", { title: name })}
      className="flex h-full min-h-0 flex-col overflow-y-auto"
    >
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-6 bottom-0 left-6 h-px bg-line-section"
        />
        <span className="relative h-[39px] w-[26px] shrink-0 overflow-hidden rounded-(--r-4) bg-surface-active">
          {poster !== null && isImageKey(poster.key) ? (
            <CachedImage imageKey={poster.key} size="list" alt="" className="size-full" />
          ) : (
            <PlaceholderPoster title={name} className="size-full" />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <h1 className="m-0 truncate text-(length:--fs-16) leading-[1.3] font-(weight:--fw-700)">
            {t("copies.page.title", { title: name })}
          </h1>
          <div className="text-(length:--fs-12) text-text-muted tabular-nums">
            {page.look_same
              ? t("copies.page.sub.same", {
                  count,
                  size: format.size(totalBytes(page)),
                })
              : t("copies.page.sub", { count, size: format.size(totalBytes(page)) })}
          </div>
        </div>
        <Button variant="ghost" shortcut="Esc" onClick={close}>
          {t("copies.page.cancel")}
        </Button>
      </div>

      <div className="flex flex-col gap-5 px-6 pt-5 pb-6">
        <div className="flex flex-col gap-1">
          {page.same_file && (
            <p
              role="note"
              className="m-0 flex max-w-[760px] items-start gap-2 text-(length:--fs-14) leading-normal"
            >
              <Icon icon={AlertTriangle} size={16} className="mt-0.5 shrink-0 text-warn" />
              <span>
                {t("copies.page.sameFile.before")}
                <Link to="/settings" search={{ section: "folders" }} className="cv-link">
                  {t("copies.page.sameFile.link")}
                </Link>
                {t("copies.page.sameFile.after")}
              </span>
            </p>
          )}
          {page.look_same && (
            <p className="m-0 flex items-center gap-2 text-(length:--fs-14) leading-normal">
              <Icon icon={Copy} size={16} className="text-gold" />
              <span className="font-(weight:--fw-600)">
                {count === 2 ? t("copies.page.same.two") : t("copies.page.same.many", { count })}
              </span>
              <span className="text-text-2">
                {t("copies.page.same.frees", { size: format.size(freedBytes(page)) })}
              </span>
            </p>
          )}
          {!page.same_file && (
            <>
              <p className="m-0 max-w-[760px] text-(length:--fs-14) leading-normal">
                {t("copies.page.intro.before")}
                <Link to="/settings" search={{ section: "deleted-files" }} className="cv-link">
                  {t("copies.page.intro.link")}
                </Link>
                {t("copies.page.intro.after")}
              </p>
              <p className="m-0 text-(length:--fs-12) text-text-muted">{destination}</p>
            </>
          )}
        </div>

        <div
          role="group"
          aria-label={t("copies.chip")}
          className="flex items-stretch gap-4 overflow-x-auto"
        >
          <Labels />
          {cards.map((card, index) => (
            <CopyCard
              key={card.copy.id}
              card={card}
              index={index}
              count={count}
              busy={busy}
              onKeep={page.same_file ? null : () => void keep(card.copy.id)}
            />
          ))}
        </div>

        <div className="flex items-center gap-3 pl-[204px]">
          {page.kept ? (
            <>
              <span className="text-(length:--fs-14) text-text">{t("copies.kept")}</span>
              <Button variant="ghost" onClick={() => void askAgain(page)}>
                {t("copies.askAgain")}
              </Button>
            </>
          ) : (
            <>
              <Button variant="ghost" className="text-text" onClick={() => void both()}>
                {count === 2 ? t("copies.keepBoth") : t("copies.keepAll")}
              </Button>
              <span className="text-(length:--fs-12) text-text-muted">
                {t("copies.keepBoth.note")}
              </span>
            </>
          )}
        </div>
      </div>
    </section>
  );
}

/** The route `/movies/$movieId/copies`. */
export function ComparePage() {
  const { movieId } = useParams({ strict: false }) as { movieId?: string };
  const id = movieId ?? "";
  const close = usePickerClose();
  useEscapeLayer("picker", true, close);
  const query = useQuery({
    queryKey: comparisonKey(id),
    queryFn: () => currentCopiesBackend().compare(id),
    staleTime: 0,
  });

  if (query.data !== undefined && query.data !== null) {
    return <Page page={query.data} movieId={id} key={id} />;
  }
  if (query.isPending) {
    return (
      <section
        aria-busy="true"
        aria-label={t("copies.loading")}
        className="flex flex-col gap-3 px-6 pt-4"
      >
        <Skeleton shape="line" width="320px" />
        <Skeleton shape="line" width="100%" height="34px" />
      </section>
    );
  }
  if (query.isError) {
    return (
      <section className="flex flex-col items-start gap-2 px-6 pt-6">
        <h1 className="type-page-title m-0">{t("copies.error")}</h1>
        <p role="alert" className="m-0 text-text-2">
          {query.error.message}
        </p>
        <Link to="/movies/$movieId" params={{ movieId: id }} className="cv-link">
          {t("copies.none.back")}
        </Link>
      </section>
    );
  }
  return (
    <section className="flex flex-col items-start gap-2 px-6 pt-6">
      <h1 className="type-page-title m-0">{t("copies.none.title")}</h1>
      <p className="m-0 text-text-2">{t("copies.none.body")}</p>
      <Link to="/movies/$movieId" params={{ movieId: id }} className="cv-link">
        {t("copies.none.back")}
      </Link>
    </section>
  );
}
