import { Copy, Send } from "lucide-react";
import { useCallback, useMemo } from "react";

import { Button } from "../../components/Button";
import { FileSize } from "../../components/FileSize";
import { MenuSeparator } from "../../components/Menu";
import { MenuItem } from "../../components/MenuItem";
import { QualityBadge } from "../../components/QualityBadge";
import { Table } from "../../components/Table";
import type { TableColumn, TableSort } from "../../components/Table";
import { Tag } from "../../components/Tag";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { sourceText, tierOf } from "./releaseSort";
import { copyMagnet, openRelease } from "./torrentActions";
import type { SourceInfo, TorrentRelease } from "./torrentBackend";
import { ageText, seedHealth } from "./torrentModel";

import "./torrentsPage.css";

/** What the library holds of the searched movie, for the hint on each release. */
export interface OwnedCopy {
  /** "2160p Dolby Vision". */
  quality: string;
  size: number | null;
}

interface ReleaseTableProps {
  releases: readonly TorrentRelease[];
  /** Every source of the search, to name the sources of a release. */
  sources: readonly SourceInfo[];
  owned: OwnedCopy | null;
  sort: TableSort;
  onSort: (sort: TableSort) => void;
}

/** How a release compares with the copy the library holds, or null when there is none. */
function Compare({ release, owned }: { release: TorrentRelease; owned: OwnedCopy | null }) {
  const format = useFormat();
  const verdict = release.upgrade;
  if (verdict !== "same" && verdict !== "better" && verdict !== "worse") return null;
  const tip =
    owned === null
      ? undefined
      : t("torrentsPage.cmp.tip", {
          quality: owned.quality,
          size: owned.size === null ? "" : format.size(owned.size),
        });
  return (
    <span className="ml-2 shrink-0" {...(tip === undefined ? {} : { title: tip })}>
      <Tag variant={verdict === "better" ? "ok" : verdict === "worse" ? "warn" : "neutral"}>
        {t(`torrentsPage.cmp.${verdict}`)}
      </Tag>
    </span>
  );
}

/**
 * The release table (spec `torrent-search` "Release table columns and sort"): the
 * shared `Table` with Quality, Release name (the hint against the owned copy after it), Size,
 * Seeders, Source and Age. Choosing a release (double-click, Enter, the row's button or the
 * right-click menu) hands its magnet link to the torrent app; nothing is downloaded here.
 */
export function ReleaseTable({ releases, sources, owned, sort, onSort }: ReleaseTableProps) {
  const format = useFormat();
  const keys = useMemo(() => releases.map((release) => release.id), [releases]);
  const byId = useMemo(() => new Map(releases.map((release) => [release.id, release])), [releases]);
  const send = useCallback(
    (id: string) => {
      const release = byId.get(id);
      if (release !== undefined) void openRelease(release);
    },
    [byId],
  );
  const selection = useListSelection({ keys, onOpen: send });
  const [chosen] = [...selection.selected];
  const only = selection.count === 1 && chosen !== undefined ? byId.get(chosen) : undefined;

  const columns = useMemo<TableColumn<TorrentRelease>[]>(
    () => [
      {
        id: "quality",
        label: t("torrentsPage.col.quality"),
        width: 104,
        cell: (release) => <QualityBadge quality={tierOf(release)} />,
      },
      {
        id: "name",
        label: t("torrentsPage.col.name"),
        width: 360,
        minWidth: 160,
        grow: true,
        cell: (release) => (
          <span className="flex min-w-0 items-center">
            <span className="cv-selectable min-w-0 truncate font-mono text-(length:--fs-13)">
              {release.name}
            </span>
            <Compare release={release} owned={owned} />
          </span>
        ),
      },
      {
        id: "size",
        label: t("torrentsPage.col.size"),
        width: 90,
        align: "end",
        sortable: true,
        cell: (release) => (release.size === null ? null : <FileSize bytes={release.size} />),
      },
      {
        id: "seeders",
        label: t("torrentsPage.col.seeders"),
        width: 100,
        align: "end",
        sortable: true,
        cell: (release) => (
          <span
            data-health={seedHealth(release.seeders)}
            className={cx(
              "font-(weight:--fw-600)",
              seedHealth(release.seeders) === 3
                ? "text-ok"
                : seedHealth(release.seeders) === 0
                  ? "text-text-muted"
                  : "text-text",
            )}
          >
            {release.seeders === null ? "" : format.number(release.seeders)}
          </span>
        ),
      },
      {
        id: "source",
        label: t("torrentsPage.col.source"),
        width: 120,
        sortable: true,
        cell: (release) => <span className="text-text-2">{sourceText(release, sources)}</span>,
      },
      {
        id: "age",
        label: t("torrentsPage.col.age"),
        width: 74,
        align: "end",
        sortable: true,
        cell: (release) => <span className="text-text-2">{ageText(release.published)}</span>,
      },
      {
        id: "send",
        label: t("torrentsPage.send"),
        header: <span className="sr-only">{t("torrentsPage.send")}</span>,
        width: 120,
        resizable: false,
        fixed: true,
        bare: true,
        align: "end",
        cell: (release) => (
          <Button
            icon={Send}
            tabIndex={-1}
            title={t("torrentsPage.send.tip")}
            className="tr-send"
            onClick={(event) => {
              event.stopPropagation();
              void openRelease(release);
            }}
            onDoubleClick={(event) => event.stopPropagation()}
          >
            {t("torrentsPage.send")}
          </Button>
        ),
      },
    ],
    [owned, sources, format],
  );

  const menu = (
    <>
      <MenuItem
        icon={Send}
        shortcut="Enter"
        {...(only === undefined ? { disabledReason: t("torrentsPage.needOne") } : {})}
        onSelect={() => only !== undefined && void openRelease(only)}
      >
        {t("torrentsPage.send")}
      </MenuItem>
      <MenuSeparator />
      <MenuItem
        icon={Copy}
        {...(only === undefined
          ? { disabledReason: t("torrentsPage.needOne") }
          : only.magnet === null
            ? { disabledReason: t("torrentsPage.noMagnet") }
            : {})}
        onSelect={() => only !== undefined && void copyMagnet(only)}
      >
        {t("torrentsPage.copy")}
      </MenuItem>
    </>
  );

  return (
    <Table
      aria-label={t("torrentsPage.table")}
      rows={releases}
      rowKey={(release) => release.id}
      columns={columns}
      selection={selection}
      storageKey="torrent-releases"
      sort={sort}
      onSortChange={onSort}
      menu={menu}
      className="min-h-0 flex-1"
    />
  );
}
