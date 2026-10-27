import { useNavigate } from "@tanstack/react-router";
import { ChevronDown } from "lucide-react";
import { useCallback, useMemo, useState } from "react";

import { Button } from "../../components/Button";
import { Checkbox } from "../../components/Checkbox";
import { FileSize } from "../../components/FileSize";
import { Menu, MenuRadioGroup, MenuRadioItem } from "../../components/Menu";
import { ProgressBar } from "../../components/ProgressBar";
import { QualityBadge } from "../../components/QualityBadge";
import { SelectionBar } from "../../components/SelectionBar";
import type { SelectionBarAction } from "../../components/SelectionBar";
import { Skeleton } from "../../components/Skeleton";
import { Table } from "../../components/Table";
import type { TableColumn, TableSort } from "../../components/Table";
import { useListSelection } from "../../components/useListSelection";
import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { usePreferOriginalTitle } from "../library/displayTitle";
import { TitleContextMenu } from "../library/TitleContextMenu";
import { useLibraryActions } from "../library/useLibraryActions";
import type { TitleTarget } from "../library/useLibraryActions";
import { useMoveToStore } from "../move-to/moveToStore";
import { BIGGEST_FIRST, sortDriveTitles, useDriveTitles } from "./titlesOfDrive";
import type { DriveTitle } from "./titlesOfDrive";
import { nameParts } from "./drivesModel";
import type { Drive } from "./drivesModel";

/** The sorts the menu offers, with the table sort each one means. */
const PRESETS: readonly { id: string; label: Parameters<typeof t>[0]; sort: TableSort }[] = [
  { id: "biggest", label: "drivesPage.sort.biggest", sort: BIGGEST_FIRST },
  { id: "smallest", label: "drivesPage.sort.smallest", sort: { column: "size", direction: "asc" } },
  { id: "title", label: "drivesPage.sort.title", sort: { column: "title", direction: "asc" } },
  { id: "newest", label: "drivesPage.sort.newest", sort: { column: "year", direction: "desc" } },
];

const COLUMN_LABEL = {
  title: "drivesPage.col.title",
  year: "drivesPage.col.year",
  quality: "drivesPage.col.quality",
  size: "drivesPage.col.size",
  folder: "drivesPage.col.folder",
} as const;

function presetOf(sort: TableSort): string {
  return (
    PRESETS.find(
      (preset) => preset.sort.column === sort.column && preset.sort.direction === sort.direction,
    )?.id ?? ""
  );
}

/** "D: Media" and what is in it: "142 titles in D:\Movies", or "57 titles in 2 folders". */
function headingOf(drive: Drive): { name: string; where: string } {
  const { head, rest } = nameParts(drive.name);
  const titles = drive.folders.reduce((sum, folder) => sum + folder.titles, 0);
  const [only] = drive.folders;
  const where =
    drive.folders.length === 0 || only === undefined
      ? ""
      : drive.folders.length === 1
        ? t("drivesPage.titles.where", { count: titles, folder: only.path })
        : t("drivesPage.titles.whereMany", { count: titles, folders: drive.folders.length });
  return { name: [head, rest].filter(Boolean).join(" "), where };
}

/**
 * The titles of the chosen drive (spec `drives` "Titles of a drive"): a table of
 * Title, Year, Quality, Size and Folder, the biggest first, sortable by any column, with desktop
 * multi-select, the right-click menu and a floating selection bar whose Move to… opens the shared
 * Move to menu with the space before and after. A running move shows its progress in the header.
 */
export function DriveTitles({ drive }: { drive: Drive }) {
  const navigate = useNavigate();
  const format = useFormat();
  const preferOriginal = usePreferOriginalTitle();
  const running = useMoveToStore((state) => state.running);
  const [sort, setSort] = useState<TableSort>(BIGGEST_FIRST);
  const heading = headingOf(drive);

  const loaded = useDriveTitles(drive, preferOriginal);
  const titles = useMemo(() => sortDriveTitles(loaded.data ?? [], sort), [loaded.data, sort]);
  const keys = useMemo(() => titles.map((title) => title.id), [titles]);
  const byId = useMemo(() => new Map(titles.map((title) => [title.id, title])), [titles]);

  const open = useCallback(
    (id: string) => {
      const title = byId.get(id);
      if (title === undefined) return;
      if (title.kind === "movie")
        void navigate({ to: "/movies/$movieId", params: { movieId: id } });
      else void navigate({ to: "/tv/$showId", params: { showId: id } });
    },
    [byId, navigate],
  );
  const selection = useListSelection({ keys, onOpen: open });

  const targets = useMemo<TitleTarget[]>(
    () =>
      [...selection.selected].flatMap((id) => {
        const title = byId.get(id);
        return title === undefined
          ? []
          : [
              {
                id,
                kind: title.kind,
                title: title.title,
                folder: title.path,
                filesMissing: title.filesMissing,
              },
            ];
      }),
    [selection.selected, byId],
  );
  const actions = useLibraryActions(targets);
  const selectedBytes = useMemo(
    () => targets.reduce((sum, target) => sum + (byId.get(target.id)?.bytes ?? 0), 0),
    [targets, byId],
  );

  const columns = useMemo<TableColumn<DriveTitle>[]>(
    () => [
      {
        id: "select",
        label: t("drivesPage.col.select"),
        header: (
          <Checkbox
            aria-label={t("drivesPage.titles.selectAll")}
            checked={selection.count > 0 && selection.count === keys.length}
            indeterminate={selection.count > 0 && selection.count < keys.length}
            onCheckedChange={(checked) => (checked ? selection.selectAll() : selection.clear())}
          />
        ),
        width: 50,
        fixed: true,
        resizable: false,
        cell: (title) => (
          // The checkbox acts as a Ctrl+click: it adds or removes this title only.
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-static-element-interactions
          <span
            className="flex items-center"
            onClick={(event) => event.stopPropagation()}
            onDoubleClick={(event) => event.stopPropagation()}
          >
            <Checkbox
              aria-label={t("drivesPage.titles.select", { title: title.title })}
              checked={selection.isSelected(title.id)}
              onCheckedChange={() => selection.click(title.id, { toggle: true })}
            />
          </span>
        ),
      },
      {
        id: "title",
        label: t(COLUMN_LABEL.title),
        width: 260,
        minWidth: 140,
        grow: true,
        sortable: true,
        cell: (title) => <span className="font-(weight:--fw-500)">{title.title}</span>,
      },
      {
        id: "year",
        label: t(COLUMN_LABEL.year),
        width: 64,
        align: "end",
        sortable: true,
        cell: (title) => (
          <span className="text-text-2">{title.year > 0 ? String(title.year) : ""}</span>
        ),
      },
      {
        id: "quality",
        label: t(COLUMN_LABEL.quality),
        width: 130,
        sortable: true,
        cell: (title) =>
          title.quality === null ? null : (
            <QualityBadge
              quality={title.quality as "2160p" | "1080p" | "720p" | "SD"}
              {...(title.hdr === null ? {} : { hdr: title.hdr })}
            />
          ),
      },
      {
        id: "size",
        label: t(COLUMN_LABEL.size),
        width: 90,
        align: "end",
        sortable: true,
        cell: (title) => (title.bytes > 0 ? <FileSize bytes={title.bytes} /> : null),
      },
      {
        id: "folder",
        label: t(COLUMN_LABEL.folder),
        width: 220,
        minWidth: 100,
        sortable: true,
        cell: (title) => (
          <span className="cv-selectable font-mono text-(length:--fs-12) text-text-2">
            {title.folder}
          </span>
        ),
      },
    ],
    [selection, keys.length],
  );

  const bar: SelectionBarAction[] = [
    {
      id: "move",
      label: t("drivesPage.bar.move"),
      icon: actions.move.def.icon,
      onSelect: actions.move.onSelect,
      ...(actions.move.disabledReason === undefined
        ? {}
        : { disabledReason: actions.move.disabledReason }),
    },
    {
      id: "delete",
      label: actions.delete.short,
      icon: actions.delete.def.icon,
      danger: true,
      onSelect: actions.delete.onSelect,
      ...(actions.delete.disabledReason === undefined
        ? {}
        : { disabledReason: actions.delete.disabledReason }),
    },
  ];

  const empty =
    drive.folders.length === 0
      ? t("drivesPage.titles.none")
      : !drive.online
        ? t("drivesPage.titles.offline")
        : t("drivesPage.titles.none");

  return (
    <div className="relative flex min-w-0 flex-1 flex-col">
      <div className="relative flex h-12 shrink-0 items-center gap-2.5 px-6">
        <span
          aria-hidden="true"
          className="absolute right-0 bottom-0 left-0 h-px bg-line-section"
        />
        <h2 className="m-0 text-(length:--fs-14) font-(weight:--fw-700) whitespace-nowrap">
          {heading.name}
        </h2>
        <span className="min-w-0 truncate text-text-muted">{heading.where}</span>
        <span className="grow" />
        {running === null ? null : (
          <span role="status" className="flex shrink-0 items-center gap-2 text-text-2">
            {t("drivesPage.moving", { target: running.target })}
            <span className="w-24">
              <ProgressBar
                label={t("drivesPage.moving", { target: running.target })}
                percent={running.percent}
              />
            </span>
            <span className="tabular-nums">{format.percent(running.percent / 100)}</span>
            {running.speed === undefined || running.paused ? null : (
              <span className="text-(length:--fs-12) text-text-muted tabular-nums">
                {running.speed}
              </span>
            )}
          </span>
        )}
        <Menu
          align="end"
          aria-label={t("drivesPage.sort.label")}
          trigger={
            <Button variant="ghost" trailingIcon={ChevronDown}>
              {presetOf(sort) === ""
                ? t("drivesPage.sort.custom", {
                    column: t(COLUMN_LABEL[sort.column as keyof typeof COLUMN_LABEL]),
                  })
                : t(
                    PRESETS.find((preset) => preset.id === presetOf(sort))?.label ??
                      "drivesPage.sort.biggest",
                  )}
            </Button>
          }
        >
          <MenuRadioGroup
            value={presetOf(sort)}
            onValueChange={(id) => {
              const preset = PRESETS.find((entry) => entry.id === id);
              if (preset !== undefined) setSort(preset.sort);
            }}
          >
            {PRESETS.map((preset) => (
              <MenuRadioItem key={preset.id} value={preset.id}>
                {t(preset.label)}
              </MenuRadioItem>
            ))}
          </MenuRadioGroup>
        </Menu>
      </div>
      {loaded.isPending && drive.folders.length > 0 ? (
        <div aria-busy="true" className="flex flex-col px-6 pt-2">
          {[0, 1, 2, 3, 4, 5].map((n) => (
            <Skeleton key={n} shape="row" />
          ))}
        </div>
      ) : (
        <Table
          aria-label={t("drivesPage.titles.table", { name: heading.name })}
          rows={titles}
          rowKey={(title) => title.id}
          columns={columns}
          selection={selection}
          storageKey="drive-titles"
          sort={sort}
          onSortChange={setSort}
          bottomRoom={selection.count > 0 ? 74 : 16}
          menu={<TitleContextMenu actions={actions} />}
          empty={empty}
          className="min-h-0 flex-1"
        />
      )}
      <SelectionBar
        count={selection.count}
        size={format.size(selectedBytes)}
        actions={bar}
        onClear={selection.clear}
      />
    </div>
  );
}
