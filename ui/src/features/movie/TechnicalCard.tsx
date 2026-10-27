import { ChevronDown, Check, TriangleAlert, Undo2 } from "lucide-react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { SectionLabel } from "../../components/SectionLabel";
import { Tag } from "../../components/Tag";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import type { MovieDetail, NfoPreview } from "../detail/detailBackend";
import { unreadText } from "../detail/heroAudio";
import type { MediaInfoRead } from "./useMediaInfoRead";
import { techSummary } from "./techModel";
import type { RenamedFrom } from "./renamedFrom";

/** Audio and subtitle lines past these are left to "Show all streams and details". */
const AUDIO_LINES = 3;
const SUBTITLE_LINES = 4;

function SpecRow({
  label,
  count,
  spec,
  children,
}: {
  label: string;
  count?: number;
  spec?: string;
  children: ReactNode;
}) {
  return (
    <div className="grid grid-cols-[86px_minmax(0,1fr)] gap-3 border-t border-solid border-line-row px-4 py-3">
      <span className="pt-px text-text-muted">
        {label}
        {count === undefined ? null : <span className="ml-1">{count}</span>}
      </span>
      <div data-spec={spec} className="min-w-0">
        {children}
      </div>
    </div>
  );
}

function More({ count }: { count: number }) {
  return count > 0 ? (
    <div className="text-(length:--fs-12) leading-[22px] text-text-muted">
      {t("movie.tech.more", { count })}
    </div>
  ) : null;
}

/** What the NFO row says about the file next to the movie. */
function NfoLine({
  nfo,
  onShow,
  fileName,
}: {
  nfo: NfoPreview | undefined;
  onShow: () => void;
  fileName: string;
}) {
  if (nfo === undefined) {
    return <span className="text-text-muted">{t("detail.nfo.loading")}</span>;
  }
  const good = nfo.sync === "in_sync";
  const key = {
    in_sync: "movie.nfo.inSync",
    differs: "movie.nfo.differs",
    missing: "movie.nfo.missing",
    not_managed: "movie.nfo.notManaged",
    no_target: "movie.nfo.noTarget",
  } as const;
  return (
    <div className="flex items-center gap-2 leading-[22px]">
      <Icon
        icon={good ? Check : TriangleAlert}
        size={14}
        className={cx("shrink-0", good ? "text-ok" : "text-warn")}
      />
      <span className="min-w-0 truncate" title={fileName}>
        {fileName}
      </span>
      <span className="shrink-0 text-text-muted">{t(key[nfo.sync])}</span>
      <button
        type="button"
        onClick={onShow}
        className="cv-link ml-auto shrink-0 text-(length:--fs-12) whitespace-nowrap"
      >
        {t("movie.nfo.show")}
      </button>
    </div>
  );
}

interface TechnicalCardProps {
  detail: MovieDetail;
  nfo: NfoPreview | undefined;
  /** The "Show all streams and details" block is open. */
  expanded: boolean;
  detailsId: string;
  onToggleDetails: () => void;
  onShowNfo: () => void;
  read: MediaInfoRead;
  renamed: RenamedFrom | null;
  onUndoRename: () => void;
}

/**
 * The Technical card of the Movie page: video, audio and subtitles as short
 * summaries, the main file with its size, container and file count, the folder, the NFO state with
 * Show NFO and the "Renamed from … · Undo" line. "Show all streams and details" opens the full
 * view in place.
 */
export function TechnicalCard({
  detail,
  nfo,
  expanded,
  detailsId,
  onToggleDetails,
  onShowNfo,
  read,
  renamed,
  onUndoRename,
}: TechnicalCardProps) {
  const format = useFormat();
  const summary = techSummary(detail, format);
  const { sheet } = summary;
  const nfoName =
    detail.files.find((file) => file.kind === "nfo")?.name ??
    (nfo?.path === null || nfo?.path === undefined
      ? "movie.nfo"
      : (nfo.path.split(/[\\/]/).pop() ?? "movie.nfo"));
  return (
    <Card className="overflow-hidden px-0 py-0" aria-label={t("movie.tech.title")}>
      <div className="flex items-center px-4 pt-3">
        <SectionLabel className="mb-3 flex-1">{t("movie.tech.title")}</SectionLabel>
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={detailsId}
          data-tech-toggle=""
          onClick={onToggleDetails}
          className="cv-link mb-3 inline-flex items-center gap-1 text-(length:--fs-12)"
        >
          {expanded ? t("movie.tech.hide") : t("movie.tech.showAll")}
          <Icon
            icon={ChevronDown}
            size={12}
            className={cx("transition-transform duration-(--dur-hover)", expanded && "rotate-180")}
          />
        </button>
      </div>

      <SpecRow label={t("movie.tech.video")} spec="video">
        {sheet.unread ? (
          <div className="flex flex-col gap-1.5">
            <span className="text-text-2">{unreadText(sheet.problem)}</span>
            <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
              <Button size="sm" disabled={read.reading} onClick={read.readNow}>
                {read.reading ? t("detail.tech.reading") : t("detail.tech.readNow")}
              </Button>
              {read.note}
            </div>
          </div>
        ) : (
          <>
            <div className="text-(length:--fs-14) font-(weight:--fw-600)">
              {summary.videoHeadline === "" ? t("movie.tech.noVideo") : summary.videoHeadline}
            </div>
            {summary.videoDetail === "" ? null : (
              <div className="mt-0.5 text-text-2">{summary.videoDetail}</div>
            )}
          </>
        )}
      </SpecRow>

      <SpecRow label={t("movie.tech.audio")} count={summary.audio.length} spec="audio">
        {summary.audio.length === 0 ? (
          <span className="text-text-muted">{t("movie.tech.none")}</span>
        ) : (
          summary.audio.slice(0, AUDIO_LINES).map((row, index) => (
            <div
              key={row.key}
              className={cx(
                "flex flex-wrap items-center gap-x-2 leading-[22px]",
                index === 0 ? "" : "text-text-2",
              )}
            >
              <span className={index === 0 ? "font-(weight:--fw-600)" : undefined}>
                {[row.format, row.channels].filter((part) => part !== "").join(" ")}
              </span>
              {index === 0 || row.bitrate === "" ? null : <span>{row.bitrate}</span>}
              {row.language === "" ? null : <span className="text-text-2">{row.language}</span>}
              {row.flags.map((flag) => (
                <Tag key={flag.id}>{flag.label}</Tag>
              ))}
            </div>
          ))
        )}
        <More count={summary.audio.length - AUDIO_LINES} />
      </SpecRow>

      <SpecRow label={t("movie.tech.subtitles")} count={summary.subtitles.length} spec="subtitles">
        {summary.subtitles.length === 0 ? (
          <span className="text-text-muted">{t("movie.tech.none")}</span>
        ) : (
          summary.subtitles.slice(0, SUBTITLE_LINES).map((row) => (
            <div key={row.key} className="flex flex-wrap items-center gap-x-2 leading-[22px]">
              <span>{row.language === "" ? t("detail.spec.unknownLanguage") : row.language}</span>
              <span className="text-text-muted">
                {row.source === "file"
                  ? t("movie.tech.external", { format: row.format })
                  : row.format}
              </span>
              {row.flags.map((flag) => (
                <Tag key={flag.id}>{flag.label}</Tag>
              ))}
            </div>
          ))
        )}
        <More count={summary.subtitles.length - SUBTITLE_LINES} />
      </SpecRow>

      <SpecRow label={t("movie.tech.file")} spec="file">
        {summary.fileName === "" ? (
          <span className="text-text-muted">{t("movie.tech.noFile")}</span>
        ) : (
          <>
            <div className="cv-selectable type-code truncate text-text" title={summary.fileName}>
              {summary.fileName}
            </div>
            <div className="mt-0.5 text-text-2">
              {[
                summary.size === null ? "" : format.size(summary.size),
                summary.container,
                t("movie.tech.filesInFolder", { count: summary.fileCount }),
              ]
                .filter((part) => part !== "")
                .join(" · ")}
            </div>
          </>
        )}
        {detail.folder === "" ? null : (
          <div className="cv-selectable type-code mt-0.5 flex text-text-muted">
            <PathText path={detail.folder} />
          </div>
        )}
      </SpecRow>

      <SpecRow label={t("movie.tech.nfo")}>
        <NfoLine nfo={nfo} onShow={onShowNfo} fileName={nfoName} />
      </SpecRow>

      {renamed === null ? null : (
        <SpecRow label={t("movie.tech.renamed")} spec="renamed">
          <div className="flex items-start gap-2.5">
            <div className="min-w-0 flex-1">
              <div className="text-(length:--fs-12) text-text-muted">
                {t("movie.tech.renamedFrom")}
              </div>
              <div className="cv-selectable type-code break-all text-text-2">{renamed.from}</div>
              <div className="mt-0.5 text-(length:--fs-12) text-text-muted">
                {format.dateTime(renamed.atMs)}
              </div>
            </div>
            <Button size="sm" icon={Undo2} onClick={onUndoRename}>
              {t("movie.tech.undo")}
            </Button>
          </div>
        </SpecRow>
      )}
    </Card>
  );
}
