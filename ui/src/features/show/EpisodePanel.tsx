import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Check, FolderOpen, Play, ScanSearch, TriangleAlert } from "lucide-react";
import { useId, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { PathText } from "../../components/PathText";
import { PlaceholderStill } from "../../components/PlaceholderPoster";
import { Icon } from "../../components/Icon";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { DetailImage } from "../detail/DetailImage";
import { mediaRows } from "../detail/mediaModel";
import { joinPath, useOpenFolder } from "../detail/useOpenFolder";
import { notify } from "../notifications/notificationStore";
import { playTitle } from "../playback/usePlay";
import { TechnicalDetails } from "../movie/TechnicalDetails";
import { showToast } from "../toasts/toastStore";
import { DetailRequestError } from "../detail/detailBackend";
import { currentTvBackend } from "../tv/tvBackend";
import type { EpisodeDetail } from "../tv/tvBackend";
import { episodeName } from "../tv/tvFormat";

function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <>
      <span className="text-text-muted">{label}</span>
      <span className="min-w-0 break-words">{children}</span>
    </>
  );
}

/** The video line of an episode: codec, size, HDR and bitrate of its first video stream. */
function videoLine(detail: EpisodeDetail): string {
  const stream = detail.videos.flatMap((video) => video.streams).find((s) => s.kind === "video");
  if (stream === undefined) return "";
  return [
    stream.codec,
    stream.width > 0 && stream.height > 0 ? `${stream.width}×${stream.height}` : "",
    stream.hdr.join(", "),
    stream.bitrate_kbps >= 1000
      ? t("detail.unit.mbps", { value: String(Math.round(stream.bitrate_kbps / 100) / 10) })
      : "",
  ]
    .filter((part) => part !== "")
    .join(" · ");
}

interface EpisodePanelProps {
  episodeId: string;
  showId: string;
}

/**
 * An episode opened in place under its row: the still, the plot, who directed and
 * wrote it, a summary of its video, audio and subtitles, its NFO state and its full path, with
 * Play, Open folder, Scrape episode and All streams. Only one is open at a time (the page holds
 * the id), and Esc closes it.
 */
export function EpisodePanel({ episodeId, showId }: EpisodePanelProps) {
  const format = useFormat();
  const queryClient = useQueryClient();
  const streamsId = useId();
  const [streams, setStreams] = useState(false);
  const backend = currentTvBackend();
  const episode = useQuery({
    queryKey: ["tv-episode", episodeId],
    queryFn: () => backend.episode(episodeId),
    staleTime: 0,
  });
  const nfo = useQuery({
    queryKey: ["tv-nfo", episodeId],
    queryFn: () => backend.episodeNfo(episodeId),
    staleTime: 0,
  });
  const detail = episode.data;
  const openFolder = useOpenFolder(detail === undefined ? "" : episodeName(detail));
  const [scraping, setScraping] = useState(false);

  if (detail === undefined) {
    return (
      <div aria-busy={episode.isPending} className="flex gap-5 p-4">
        <Skeleton shape="tile" aspect="16 / 9" className="w-[240px] shrink-0" />
        <div className="flex flex-1 flex-col gap-2">
          <Skeleton shape="line" width="80%" />
          <Skeleton shape="line" width="60%" />
        </div>
      </div>
    );
  }

  const rows = mediaRows(detail, format);
  const name = episodeName(detail);
  const audio = rows.audio
    .map((row) => [row.format, row.channels, row.language].filter((p) => p !== "").join(" "))
    .join(" · ");
  const subtitles = [...new Set(rows.subtitles.map((row) => row.language).filter((l) => l !== ""))];
  const file = detail.videos[0];
  const missingFiles = detail.availability === "missing" || detail.availability === "offline";

  const scrape = () => {
    setScraping(true);
    void backend
      .scrapeEpisode(episodeId)
      .then(() => {
        showToast({ message: t("show.episode.scraped", { title: name }) });
        void queryClient.invalidateQueries();
      })
      .catch((error: unknown) => {
        const noMatch = error instanceof DetailRequestError && error.code === "no_match";
        notify({
          severity: "error",
          title: noMatch
            ? t("show.episode.noMatch", { title: name })
            : t("show.episode.scrapeFailed", { title: name }),
          detail: error instanceof Error ? error.message : String(error),
        });
      })
      .finally(() => setScraping(false));
  };

  return (
    <div data-testid="episode-panel" data-show={showId} className="px-4 pb-4">
      <div className="flex gap-5 pt-4">
        <div className="relative aspect-video w-[240px] shrink-0 overflow-hidden rounded-(--r-10) bg-surface-raised">
          {detail.thumb === null ? (
            <PlaceholderStill title={detail.show_title} index={detail.number} />
          ) : (
            <DetailImage image={detail.thumb} size="hero" className="object-cover" />
          )}
        </div>
        <div className="flex min-w-0 flex-1 flex-col gap-2.5">
          <p className="m-0 leading-[1.55]">
            {detail.plot === "" ? t("show.episode.noPlot") : detail.plot}
          </p>
          <dl className="m-0 grid grid-cols-[96px_minmax(0,1fr)_96px_minmax(0,1fr)] gap-x-3 gap-y-[5px]">
            <Fact label={t("show.episode.directed")}>{format.list(detail.directors) || "–"}</Fact>
            <Fact label={t("show.episode.written")}>{format.list(detail.writers) || "–"}</Fact>
            <Fact label={t("movie.tech.video")}>{videoLine(detail) || "–"}</Fact>
            <Fact label={t("movie.tech.audio")}>{audio || "–"}</Fact>
            <Fact label={t("movie.tech.subtitles")}>{format.list(subtitles) || "–"}</Fact>
            <Fact label={t("movie.tech.nfo")}>
              {nfo.data === undefined ? (
                "…"
              ) : (
                <span
                  className={`inline-flex items-center gap-1.5 ${nfo.data.sync === "in_sync" ? "text-ok" : "text-warn"}`}
                >
                  <Icon icon={nfo.data.sync === "in_sync" ? Check : TriangleAlert} size={14} />
                  {t(
                    nfo.data.sync === "in_sync"
                      ? "show.episode.nfoOk"
                      : nfo.data.sync === "missing"
                        ? "show.episode.nfoMissing"
                        : "show.episode.nfoDiffers",
                  )}
                </span>
              )}
            </Fact>
          </dl>
          {file === undefined ? null : (
            <div className="cv-selectable type-code flex text-text-2">
              <PathText path={joinPath(detail.folder, file.name)} />
            </div>
          )}
        </div>
        <div className="flex w-[132px] shrink-0 flex-col gap-1.5">
          <Button
            size="sm"
            icon={Play}
            disabledReason={
              missingFiles || file === undefined ? t("show.episode.noFile") : undefined
            }
            onClick={() => void playTitle(episodeId, name)}
          >
            {t("title.action.play")}
          </Button>
          <Button
            size="sm"
            icon={FolderOpen}
            disabledReason={detail.folder === "" ? t("title.openFolder.none") : undefined}
            onClick={() => openFolder(detail.folder)}
          >
            {t("title.action.openFolder")}
          </Button>
          <Button size="sm" icon={ScanSearch} disabled={scraping} onClick={scrape}>
            {t("show.episode.scrape")}
          </Button>
          <button
            type="button"
            aria-expanded={streams}
            aria-controls={streamsId}
            onClick={() => setStreams((value) => !value)}
            className="cv-link mt-0.5 self-start text-(length:--fs-12)"
          >
            {streams ? t("show.episode.hideStreams") : t("show.episode.allStreams")}
          </button>
        </div>
      </div>
      <TechnicalDetails
        id={streamsId}
        embedded
        detail={{ ...detail, title: name, media_source: "", edition: "" }}
        open={streams}
        onHide={() => setStreams(false)}
      />
    </div>
  );
}
