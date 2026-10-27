import { ChevronUp, Copy } from "lucide-react";
import type { ReactNode } from "react";

import type { DetailStreamDto, DetailVideoDto } from "../../bindings";
import { Button } from "../../components/Button";
import { Card } from "../../components/Card";
import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { t } from "../../i18n/catalog";
import type { Formatters } from "../../i18n/format";
import { useFormat } from "../../i18n/useFormat";
import type { MovieDetail } from "../detail/detailBackend";
import { languageName } from "../detail/detailFormat";
import { mediaRows } from "../detail/mediaModel";
import { bitrateLabel } from "../detail/streams";
import { copyText } from "../disclosure/copy";
import { durationText, hdrFacts, hdrLabel, overallBitrate, totalVideoSize } from "./techModel";

/**
 * What the expanded view reads: a movie, or an episode with its own file facts (`media_source`
 * and `edition` are empty for an episode).
 */
export type TechnicalSource = Pick<
  MovieDetail,
  "title" | "videos" | "files" | "subtitles" | "hero_audio" | "folder" | "media_source" | "edition"
> & { year?: number };

/** One table of the expanded view: the heading cells and the rows of text. */
interface TextTable {
  head: string[];
  rows: string[][];
}

const dash = "–";
const yes = () => t("movie.tech.yes");
const no = () => t("movie.tech.no");

function videoTable(
  streams: { stream: DetailStreamDto; index: number }[],
  format: Formatters,
): TextTable {
  return {
    head: [
      "#",
      t("movie.tech.col.codec"),
      t("movie.tech.col.resolution"),
      t("movie.tech.col.frameRate"),
      t("movie.tech.col.bitDepth"),
      t("movie.tech.col.hdr"),
      t("movie.tech.col.bitrate"),
      t("movie.tech.col.default"),
    ],
    rows: streams.map(({ stream, index }) => [
      String(index),
      stream.codec || dash,
      stream.width > 0 && stream.height > 0 ? `${stream.width}×${stream.height}` : dash,
      stream.frame_rate !== null && stream.frame_rate > 0
        ? t("detail.unit.fps", {
            value: format.number(Math.round(stream.frame_rate * 1000) / 1000),
          })
        : dash,
      stream.bit_depth > 0 ? t("detail.unit.bit", { value: stream.bit_depth }) : dash,
      stream.hdr.length === 0 ? dash : hdrLabel(stream),
      stream.bitrate_kbps > 0 ? bitrateLabel(stream.bitrate_kbps, format) : dash,
      stream.flags.includes("default") ? yes() : no(),
    ]),
  };
}

function audioTable(
  streams: { stream: DetailStreamDto; index: number }[],
  format: Formatters,
): TextTable {
  return {
    head: [
      "#",
      t("movie.tech.col.codec"),
      t("movie.tech.col.channels"),
      t("movie.tech.col.sampleRate"),
      t("movie.tech.col.bitDepth"),
      t("movie.tech.col.bitrate"),
      t("movie.tech.col.language"),
      t("movie.tech.col.title"),
      t("movie.tech.col.default"),
      t("movie.tech.col.forced"),
    ],
    rows: streams.map(({ stream, index }) => [
      String(index),
      stream.codec || dash,
      stream.channels > 0 ? t("detail.unit.channels", { count: stream.channels }) : dash,
      stream.sample_rate_hz > 0
        ? t("detail.unit.kHz", { value: format.number(stream.sample_rate_hz / 1000) })
        : dash,
      stream.bit_depth > 0 ? t("detail.unit.bit", { value: stream.bit_depth }) : dash,
      stream.bitrate_kbps > 0 ? bitrateLabel(stream.bitrate_kbps, format) : dash,
      languageName(stream.language, format.locale) || dash,
      stream.title || dash,
      stream.flags.includes("default") ? yes() : no(),
      stream.flags.includes("forced") ? yes() : no(),
    ]),
  };
}

function numbered(videos: readonly DetailVideoDto[], kind: string) {
  let index = 0;
  return videos.flatMap((video) =>
    video.streams
      .filter((stream) => stream.kind === kind)
      .map((stream) => {
        index += 1;
        return { stream, index };
      }),
  );
}

/** Every table of the expanded view as data, so the screen and "Copy as text" say the same. */
function sections(detail: TechnicalSource, format: Formatters) {
  const subtitles = mediaRows(detail, format).subtitles;
  const embedded = subtitles.filter((row) => row.source === "embedded").length;
  const subtitleTable: TextTable = {
    head: [
      "#",
      t("movie.tech.col.format"),
      t("movie.tech.col.language"),
      t("movie.tech.col.title"),
      t("movie.tech.col.source"),
      t("movie.tech.col.default"),
      t("movie.tech.col.forced"),
    ],
    rows: subtitles.map((row) => [
      row.source === "file" ? dash : String(row.number),
      row.format || dash,
      row.language || dash,
      row.name || dash,
      row.source === "file" ? row.fileName : t("movie.tech.embedded"),
      row.flags.some((flag) => flag.id === "default") ? yes() : no(),
      row.flags.some((flag) => flag.id === "forced") ? yes() : no(),
    ]),
  };
  const filesTable: TextTable = {
    head: [t("movie.tech.col.name"), t("movie.tech.col.kind"), t("movie.tech.col.size")],
    rows: [
      ...detail.videos.map((video) => [
        video.name,
        t("movie.tech.kind.video"),
        video.size_bytes === null ? dash : format.size(video.size_bytes),
      ]),
      ...detail.files.map((file) => [
        file.name,
        file.kind.replaceAll("_", " "),
        file.size_bytes === null ? dash : format.size(file.size_bytes),
      ]),
    ],
  };
  return {
    video: videoTable(numbered(detail.videos, "video"), format),
    audio: audioTable(numbered(detail.videos, "audio"), format),
    subtitles: subtitleTable,
    embedded,
    external: subtitles.length - embedded,
    files: filesTable,
  };
}

/** The container facts of one file, as label and value pairs. */
function containerFacts(video: DetailVideoDto, detail: TechnicalSource, format: Formatters) {
  const facts: [string, string][] = [
    [t("movie.tech.fact.format"), video.container],
    [
      t("movie.tech.fact.size"),
      video.size_bytes === null
        ? ""
        : `${format.size(video.size_bytes)} (${t("movie.tech.bytes", { count: video.size_bytes })})`,
    ],
    [t("movie.tech.fact.duration"), durationText(video.duration_secs)],
    [t("movie.tech.fact.bitrate"), overallBitrate(video.size_bytes, video.duration_secs, format)],
    [t("movie.tech.fact.source"), detail.media_source.replaceAll("_", " ")],
    [t("movie.tech.fact.edition"), detail.edition.replaceAll("_", " ")],
  ];
  return facts.filter(([, value]) => value !== "");
}

/** The HDR facts of every video stream that has some, one section per stream. */
function hdrSectionsOf(detail: TechnicalSource, format: Formatters) {
  return detail.videos.flatMap((video) =>
    video.streams.flatMap((stream, index) => {
      const facts = stream.kind === "video" ? hdrFacts(stream, format) : [];
      return facts.length === 0
        ? []
        : [{ key: `${video.name}:${index}`, label: hdrLabel(stream), facts }];
    }),
  );
}

/** The whole expanded view as plain text, for "Copy as text". */
function technicalText(detail: TechnicalSource, format: Formatters): string {
  const all = sections(detail, format);
  const lines: string[] = [
    detail.year === undefined ? detail.title : `${detail.title} (${detail.year})`,
    "",
  ];
  for (const video of detail.videos) {
    lines.push(`${t("movie.tech.container")}: ${video.name}`);
    for (const [label, value] of containerFacts(video, detail, format)) {
      lines.push(`  ${label}: ${value}`);
    }
    lines.push("");
  }
  const block = (title: string, table: TextTable) => {
    lines.push(title, table.head.join("\t"), ...table.rows.map((row) => row.join("\t")), "");
  };
  block(t("movie.tech.video"), all.video);
  for (const { label, facts } of hdrSectionsOf(detail, format)) {
    lines.push(
      `${t("movie.tech.hdr.title")}: ${label}`,
      ...facts.map((fact) => `  ${fact.label}: ${fact.value}`),
      "",
    );
  }
  block(t("movie.tech.audio"), all.audio);
  block(t("movie.tech.subtitles"), all.subtitles);
  lines.push(detail.folder);
  block(t("movie.tech.files"), all.files);
  return lines.join("\n").trimEnd();
}

function Section({
  title,
  note,
  children,
  aside,
}: {
  title: string;
  note?: ReactNode;
  aside?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card className="overflow-hidden px-0 py-0">
      <div className="flex items-center gap-2.5 border-b border-solid border-line-section px-4 py-[11px]">
        <b className="text-(length:--fs-13)">{title}</b>
        {note === undefined ? null : <span className="min-w-0 text-text-muted">{note}</span>}
        <span className="flex-1" />
        {aside}
      </div>
      {children}
    </Card>
  );
}

function Grid({ table, mono = [] }: { table: TextTable; mono?: number[] }) {
  return (
    <div className="overflow-x-auto">
      <table className="w-full border-separate border-spacing-0 text-(length:--fs-13)">
        <thead>
          <tr>
            {table.head.map((cell, index) => (
              <th
                key={`${cell}:${index}`}
                scope="col"
                className="h-(--h-thead) border-b border-solid border-line-section px-3 text-left text-(length:--fs-12) font-(weight:--fw-600) whitespace-nowrap text-text-muted first:pl-4 last:pr-4"
              >
                {cell}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {table.rows.map((row, rowIndex) => (
            <tr key={rowIndex}>
              {row.map((cell, index) => (
                <td
                  key={index}
                  className={`h-(--h-row) border-b border-solid border-line-row px-3 whitespace-nowrap first:pl-4 last:pr-4 ${
                    index === 0 ? "text-text-muted" : ""
                  } ${mono.includes(index) ? "type-code cv-selectable" : ""} ${
                    rowIndex === table.rows.length - 1 ? "border-b-0" : ""
                  }`}
                >
                  {cell}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

interface TechnicalDetailsProps {
  detail: TechnicalSource;
  /** Inside another panel (an episode): no page inset and a smaller heading. */
  embedded?: boolean;
  id: string;
  open: boolean;
  onHide: () => void;
}

/**
 * "Show all streams and details": the container, video, audio and subtitle
 * tables and the list of files in the folder, in place under the Technical card, with Copy as text
 * and Hide details. It opens with `.cv-expand`, so it leaves the tab order while closed.
 * Subtitle tracks are listed as media info reads them; nothing here changes a file.
 */
export function TechnicalDetails({
  detail,
  id,
  open,
  onHide,
  embedded = false,
}: TechnicalDetailsProps) {
  const format = useFormat();
  const all = sections(detail, format);
  const totalFiles = detail.videos.length + detail.files.length;
  const hdrSections = hdrSectionsOf(detail, format);
  return (
    <div id={id} className="cv-expand" data-open={open ? "true" : "false"}>
      <div>
        <section
          aria-label={t("movie.tech.details")}
          data-testid="technical-details"
          className={
            embedded ? "flex flex-col gap-4 pt-4" : "flex flex-col gap-4 px-(--inset) pt-7"
          }
        >
          <div className="flex items-center gap-2.5">
            <h2 className="m-0 flex-1 text-(length:--fs-16) font-(weight:--fw-700)">
              {t("movie.tech.details")}
            </h2>
            <Button
              variant="ghost"
              icon={Copy}
              onClick={() =>
                void copyText(
                  technicalText(detail, format),
                  t("movie.tech.copied"),
                  t("path.copyFailed"),
                )
              }
            >
              {t("movie.tech.copy")}
            </Button>
            <button
              type="button"
              onClick={onHide}
              className="cv-link inline-flex items-center gap-1 text-(length:--fs-13)"
            >
              {t("movie.tech.hideDetails")}
              <Icon icon={ChevronUp} size={12} />
            </button>
          </div>

          {detail.videos.map((video) => (
            <Section key={video.name} title={t("movie.tech.container")} note={video.name}>
              {video.analysed ? (
                <dl className="m-0 grid grid-cols-4">
                  {containerFacts(video, detail, format).map(([label, value]) => (
                    <div
                      key={label}
                      className="border-r border-b border-solid border-line-row px-4 py-2.5 [&:nth-child(4n)]:border-r-0"
                    >
                      <dt className="mb-0.5 text-(length:--fs-12) text-text-muted">{label}</dt>
                      <dd className="cv-selectable m-0">{value}</dd>
                    </div>
                  ))}
                </dl>
              ) : (
                <p className="m-0 px-4 py-3 text-text-muted">{t("detail.files.noMediaInfo")}</p>
              )}
            </Section>
          ))}

          <Section
            title={t("movie.tech.video")}
            note={t("movie.tech.tracks", { count: all.video.rows.length })}
          >
            <Grid table={all.video} />
          </Section>
          {hdrSections.map(({ key, label, facts }) => (
            <Section key={key} title={t("movie.tech.hdr.title")} note={label}>
              <dl className="m-0 grid grid-cols-4" data-testid="hdr-details">
                {facts.map((fact) => (
                  <div
                    key={fact.label}
                    className="border-r border-b border-solid border-line-row px-4 py-2.5 [&:nth-child(4n)]:border-r-0"
                  >
                    <dt className="mb-0.5 text-(length:--fs-12) text-text-muted">{fact.label}</dt>
                    <dd className="cv-selectable m-0">{fact.value}</dd>
                  </div>
                ))}
              </dl>
            </Section>
          ))}
          <Section
            title={t("movie.tech.audio")}
            note={t("movie.tech.tracks", { count: all.audio.rows.length })}
          >
            <Grid table={all.audio} />
          </Section>
          <Section
            title={t("movie.tech.subtitles")}
            note={t("movie.tech.subtitleTracks", {
              count: all.subtitles.rows.length,
              embedded: all.embedded,
              external: all.external,
            })}
          >
            <Grid table={all.subtitles} mono={[4]} />
          </Section>
          <Section
            title={t("movie.tech.files")}
            note={
              detail.folder === "" ? undefined : (
                <span className="type-code cv-selectable flex">
                  <PathText path={detail.folder} />
                </span>
              )
            }
            aside={
              <span className="text-text-muted">
                {t("movie.tech.fileTotal", {
                  count: totalFiles,
                  size: format.size(
                    (totalVideoSize(detail.videos) ?? 0) +
                      detail.files.reduce((sum, file) => sum + (file.size_bytes ?? 0), 0),
                  ),
                })}
              </span>
            }
          >
            <Grid table={all.files} mono={[0]} />
          </Section>
        </section>
      </div>
    </div>
  );
}
