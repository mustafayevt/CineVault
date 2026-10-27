import { useNavigate } from "@tanstack/react-router";
import { ChevronDown, ChevronRight } from "lucide-react";
import { useEffect, useRef, useState } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { useEscapeLayer } from "../shell/escStack";
import type { ShowDetail, TvSeason } from "../tv/tvBackend";
import { episodeName, seasonTitle } from "../tv/tvFormat";
import { EpisodePanel } from "./EpisodePanel";
import { distinctSize, onDisk, rowsOf } from "./showModel";
import type { ShowRow } from "./showModel";

/** Episode columns: #, title, aired, runtime, quality, size, status. */
const COLUMNS = "grid-cols-[44px_minmax(0,1fr)_110px_70px_120px_80px_110px]";

function runtime(minutes: number): string {
  return minutes > 0 ? t("detail.runtime.minutes", { minutes }) : "–";
}

/** How long an episode panel keeps its place while it closes, so the height can animate. */
const CLOSE_MS = 200;

interface EpisodeListProps {
  detail: ShowDetail;
  season: number;
  openId: string | null;
  onOpen: (id: string | null) => void;
}

/**
 * The episodes of one season as a table: a 30px header and 34px rows with the same hairlines as
 * every table. A row of the library opens in place under itself (`.cv-expand`); an episode the
 * library lacks is a muted row that says Missing in the warning colour, never an error. Only one
 * episode is open at a time and Esc closes it.
 */
function EpisodeList({ detail, season, openId, onOpen }: EpisodeListProps) {
  const format = useFormat();
  const rows = rowsOf(detail, season);
  // The panel that is closing stays mounted until its height has animated away.
  const [closing, setClosing] = useState<string | null>(null);
  const [shown, setShown] = useState<string | null>(null);
  const previous = useRef<string | null>(null);
  useEffect(() => {
    const before = previous.current;
    previous.current = openId;
    if (before !== null && before !== openId) {
      setClosing(before);
      const timer = window.setTimeout(() => {
        setClosing(null);
        setShown(null);
      }, CLOSE_MS);
      return () => window.clearTimeout(timer);
    }
  }, [openId]);
  // The opening panel mounts closed and flips open on the next frame, so its height animates.
  useEffect(() => {
    if (openId === null) return;
    const frame = window.requestAnimationFrame(() => setShown(openId));
    return () => window.cancelAnimationFrame(frame);
  }, [openId]);

  useEscapeLayer(
    "selection",
    openId !== null && rows.some((r) => r.kind === "episode" && r.episode.id === openId),
    () => {
      onOpen(null);
    },
  );

  const statusOf = (row: ShowRow) =>
    row.kind === "episode"
      ? onDisk(row.episode)
        ? { text: t("show.status.onDisk"), tone: "text-ok" }
        : { text: t("show.status.noFile"), tone: "text-warn" }
      : row.kind === "missing"
        ? { text: t("show.status.missing"), tone: "text-warn" }
        : { text: t("show.status.unaired"), tone: "text-text-muted" };

  return (
    <div role="table" aria-label={t("show.episodes.label", { season: seasonTitle(season) })}>
      <div
        role="row"
        className={cx(
          "grid h-(--h-thead) items-center border-b border-solid border-line-section px-3.5 text-(length:--fs-12) font-(weight:--fw-600) text-text-muted",
          COLUMNS,
        )}
      >
        <span role="columnheader">#</span>
        <span role="columnheader">{t("show.col.title")}</span>
        <span role="columnheader">{t("show.col.aired")}</span>
        <span role="columnheader">{t("show.col.runtime")}</span>
        <span role="columnheader">{t("show.col.quality")}</span>
        <span role="columnheader" className="text-right">
          {t("show.col.size")}
        </span>
        <span role="columnheader" className="text-right">
          {t("show.col.status")}
        </span>
      </div>
      {rows.map((row) => {
        const status = statusOf(row);
        const episode = row.kind === "episode" ? row.episode : null;
        const open = episode !== null && episode.id === openId;
        const keep = episode !== null && (open || episode.id === closing);
        const name =
          episode !== null
            ? episodeName(episode)
            : episodeName({
                title: row.kind === "episode" ? "" : row.missing.title,
                number: row.number,
              });
        const muted = episode === null || !onDisk(episode);
        const date =
          episode !== null
            ? episode.first_aired
            : row.kind === "episode"
              ? null
              : row.missing.first_aired;
        const cells = (
          <>
            <span className="text-text-muted tabular-nums">{row.number}</span>
            <span
              className={cx(
                "min-w-0 truncate font-(weight:--fw-500)",
                muted && "text-text-disabled",
              )}
            >
              {episode !== null ? (
                <button
                  type="button"
                  aria-expanded={open}
                  onClick={(event) => {
                    event.stopPropagation();
                    onOpen(open ? null : episode.id);
                  }}
                  className="min-w-0 max-w-full truncate bg-transparent p-0 text-left text-inherit"
                >
                  {name}
                </button>
              ) : (
                name
              )}
            </span>
            <span className="truncate text-text-2">
              {date === null ? "–" : format.date(`${date}T00:00:00`)}
            </span>
            <span className="text-text-2">
              {episode === null ? "–" : runtime(episode.runtime_minutes)}
            </span>
            <span className="truncate text-text-2">
              {episode === null || episode.resolution === "" ? "–" : episode.resolution}
            </span>
            <span className="text-right tabular-nums">
              {episode === null || episode.size_bytes === null || !onDisk(episode)
                ? "–"
                : format.size(episode.size_bytes)}
            </span>
            <span className={cx("text-right", status.tone)}>{status.text}</span>
          </>
        );
        return (
          <div
            key={`${row.kind}:${row.number}:${episode?.id ?? ""}`}
            data-testid={episode === null ? undefined : "episode-row"}
            data-state={open ? "open" : undefined}
            className={cx(
              "overflow-hidden",
              keep &&
                "my-1 rounded-(--r-10) border border-solid border-line-edge bg-surface-card shadow-[inset_2px_0_0_var(--gold)]",
            )}
          >
            {/* The whole row opens the episode; its title button is the keyboard route. */}
            {/* eslint-disable jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus */}
            <div
              role="row"
              onClick={episode === null ? undefined : () => onOpen(open ? null : episode.id)}
              className={cx(
                "grid h-(--h-row) items-center px-3.5 text-(length:--fs-13)",
                COLUMNS,
                !keep && "border-b border-solid border-line-row",
                keep && "border-b border-solid border-line-row",
                episode !== null && "cv-hoverable cursor-pointer hover:bg-surface-card",
              )}
            >
              {cells}
            </div>
            {/* eslint-enable jsx-a11y/click-events-have-key-events, jsx-a11y/interactive-supports-focus */}
            {episode === null || !keep ? null : (
              <div
                className="cv-expand"
                data-open={open && shown === episode.id ? "true" : "false"}
              >
                <div>
                  <EpisodePanel episodeId={episode.id} showId={detail.id} />
                </div>
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

interface SeasonSectionProps {
  detail: ShowDetail;
  season: TvSeason;
  open: boolean;
  onToggle: () => void;
  openEpisode: string | null;
  onOpenEpisode: (id: string | null) => void;
}

/**
 * One season of the show page: a 44px header card with a chevron, "Season 2", its year and
 * episode count, "N missing" in the warning colour (or Complete, or Not on disk), its size and
 * Artwork, and the episode table under it when open. The specials are a section like any other.
 */
export function SeasonSection({
  detail,
  season,
  open,
  onToggle,
  openEpisode,
  onOpenEpisode,
}: SeasonSectionProps) {
  const format = useFormat();
  const navigate = useNavigate();
  const episodes = detail.episodes.filter((episode) => episode.season === season.number);
  const size = distinctSize(episodes);
  const year = season.first_aired === null ? "" : season.first_aired.slice(0, 4);
  const title = seasonTitle(season.number);
  const counted = season.episodes + season.missing;
  const note = [
    year,
    season.number === 0
      ? t("show.season.specialsCount", { count: season.episodes + season.missing })
      : t("show.season.episodes", { count: counted > 0 ? counted : season.episodes }),
  ]
    .filter((part) => part !== "")
    .join(" · ");
  const panelId = `season-${season.number}`;
  return (
    <section aria-label={title} data-season={season.number}>
      <div className="flex h-11 items-center gap-3 rounded-(--r-10) border border-solid border-line-section bg-surface-card px-3.5">
        <button
          type="button"
          aria-expanded={open}
          aria-controls={panelId}
          onClick={onToggle}
          className="cv-hoverable flex min-w-0 flex-1 items-center gap-3 self-stretch rounded-(--r-6) bg-transparent text-left text-inherit"
        >
          <Icon icon={open ? ChevronDown : ChevronRight} />
          <span className="text-(length:--fs-14) font-(weight:--fw-700)">{title}</span>
          <span className="text-text-muted">{note}</span>
          {season.missing > 0 ? (
            <Tag variant="warn">{t("show.season.missing", { count: season.missing })}</Tag>
          ) : season.episodes > 0 ? (
            <Tag variant="ok">{t("show.season.complete")}</Tag>
          ) : (
            <Tag>{t("show.season.notOnDisk")}</Tag>
          )}
          <span className="flex-1" />
          {size === null ? null : <span className="text-text-muted">{format.size(size)}</span>}
        </button>
        {season.number === 0 ? null : (
          <Button
            variant="ghost"
            size="sm"
            onClick={() =>
              void navigate({
                to: "/artwork",
                search: { ids: season.id ?? detail.id, type: "poster" } as never,
              })
            }
          >
            {t("show.season.artwork")}
          </Button>
        )}
      </div>
      <div id={panelId} className="cv-expand" data-open={open ? "true" : "false"}>
        <div>
          <div className="pt-2.5 pb-1">
            <EpisodeList
              detail={detail}
              season={season.number}
              openId={openEpisode}
              onOpen={onOpenEpisode}
            />
          </div>
        </div>
      </div>
    </section>
  );
}
