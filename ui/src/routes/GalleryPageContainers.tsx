import {
  Download,
  Film,
  FolderPen,
  HardDrive,
  History,
  RefreshCw,
  ScanSearch,
  Settings,
  Trash2,
} from "lucide-react";
import { useEffect, useMemo, useState } from "react";

import { IconButton } from "../components/Button";
import { Card, SectionLabel } from "../components/Card";
import { NavItem } from "../components/NavItem";
import { PosterTile } from "../components/PosterTile";
import { ProgressBar } from "../components/ProgressBar";
import { ProgressCard } from "../components/ProgressCard";
import { QualityBadge } from "../components/QualityBadge";
import { ScoreBadge } from "../components/ScoreBadge";
import { SelectionBar } from "../components/SelectionBar";
import type { SelectionBarAction } from "../components/SelectionBar";
import { Skeleton } from "../components/Skeleton";
import { SpaceBar } from "../components/SpaceBar";
import { StageBadge } from "../components/StageBadge";
import type { StageBadgeKind } from "../components/StageBadge";
import { Table } from "../components/Table";
import type { TableColumn, TableSort } from "../components/Table";
import { Toast, ToastStack } from "../components/Toast";
import { useListSelection } from "../components/useListSelection";
import { cx } from "../lib/cx";
import { useColumnLabel } from "./galleryTheme";
import { Group, Row, Stage } from "./GalleryPageParts";

const DRIVES = [
  { name: "C: System", note: "41 GB free", used: 0.96, state: "low" as const },
  { name: "D: Media", note: "1.21 TB free", used: 0.83, state: "normal" as const },
  { name: "E: Archive", note: "5.81 TB free", used: 0.47, state: "normal" as const },
  { name: "Z: NAS", note: "Offline", used: 0, state: "offline" as const },
];

/** The sidebar panel as a specimen: nav items, Drives, the progress card and the icon row. */
function Sidebar({ collapsed }: { collapsed: boolean }) {
  return (
    <div
      className={cx(
        "flex flex-col gap-0.5 rounded-(--r-14) border border-solid border-line-edge bg-surface-floating p-2 shadow-floating",
        collapsed ? "w-14 items-center" : "w-[216px]",
      )}
    >
      <NavItem icon={Film} label="Library" active collapsed={collapsed} />
      <NavItem icon={ScanSearch} label="Scrape" count={12} collapsed={collapsed} />
      <NavItem icon={FolderPen} label="Rename" count={71} collapsed={collapsed} />
      <NavItem
        icon={HardDrive}
        label="Drives"
        count="1 low"
        countTone="warn"
        collapsed={collapsed}
      />
      <NavItem icon={Download} label="Torrents" collapsed={collapsed} />
      {!collapsed && (
        <>
          <div className="px-2.5 pt-4.5 pb-1.5">
            <SectionLabel className="mb-0">Drives</SectionLabel>
          </div>
          {DRIVES.map((drive) => (
            <div
              key={drive.name}
              className={cx(
                "flex flex-col gap-1.5 rounded-(--r-6) px-2.5 py-[7px]",
                drive.state === "offline" && "text-text-disabled",
              )}
            >
              <span className="flex justify-between gap-2 text-(length:--fs-12) whitespace-nowrap">
                <span className="font-(weight:--fw-600)">{drive.name}</span>
                <span
                  className={
                    drive.state === "low"
                      ? "text-warn"
                      : drive.state === "offline"
                        ? "text-text-disabled"
                        : "text-text-muted"
                  }
                >
                  {drive.note}
                </span>
              </span>
              <SpaceBar
                used={drive.used}
                state={drive.state}
                label={`${drive.name}, ${Math.round(drive.used * 100)}% used`}
              />
            </div>
          ))}
        </>
      )}
      <div className="h-3" />
      <ProgressCard
        title="Scraping 3 of 25"
        percent={12}
        detail="Heat (1995): writing artwork"
        collapsed={collapsed}
      />
      <div
        className={cx(
          "mt-3 flex border-t border-solid border-line-section pt-2",
          collapsed ? "flex-col items-center gap-1" : "justify-around",
        )}
      >
        <IconButton aria-label="History" icon={History} />
        <IconButton aria-label="Settings" icon={Settings} />
      </div>
    </div>
  );
}

function SidebarParts() {
  return (
    <Group
      title="NavItem, SectionLabel, SpaceBar, ProgressCard"
      note="The sidebar panel on the floating surface at radius 14: the progress card (radius 6, well colour) sits 8px inside it. Hover a row to see the hover rung."
    >
      <div className="flex flex-wrap items-start gap-6">
        <Stage className="p-2">
          <Sidebar collapsed={false} />
        </Stage>
        <Stage className="p-2">
          <Sidebar collapsed />
        </Stage>
      </div>
      <Row label="NavItem states (hover and pressed forced)" className="items-start">
        <div className="flex w-[216px] flex-col gap-0.5 rounded-(--r-14) border border-solid border-line-edge bg-surface-floating p-2 shadow-floating">
          <NavItem icon={Film} label="Rest" />
          <NavItem icon={Film} label="Hover" className="bg-surface-floating-hover! text-text!" />
          <NavItem icon={Film} label="Pressed" className="bg-surface-pressed! text-text!" />
          <NavItem icon={Film} label="Active" active count={4} />
          <NavItem icon={Film} label="Focus" className="outline-2 outline-offset-2 outline-text" />
        </div>
      </Row>
    </Group>
  );
}

function Bars() {
  return (
    <Group
      title="ProgressBar and SpaceBar"
      note="3px grooves. Gold is for progress, never for space; low space is the warning bar; an offline drive is a bare groove."
    >
      <Row label="Progress: 0, 40, 100, network, 6px, unknown length" className="items-end">
        <div className="grid w-full max-w-md gap-3">
          <ProgressBar label="Empty" percent={0} />
          <ProgressBar label="Scanning" percent={40} />
          <ProgressBar label="Done" percent={100} />
          <ProgressBar label="Providers" percent={65} tone="info" />
          <ProgressBar label="Medium" percent={65} size="md" />
          <ProgressBar label="Waiting" percent={0} indeterminate />
        </div>
      </Row>
      <Row label="Space: normal, low, offline">
        <div className="grid w-full max-w-md gap-3">
          <SpaceBar used={0.47} label="E: Archive, 47% used" />
          <SpaceBar used={0.96} label="C: System, 96% used" state="low" />
          <SpaceBar used={0} label="Z: NAS, offline" state="offline" />
        </div>
      </Row>
    </Group>
  );
}

function Cards() {
  return (
    <Group
      title="Card and SectionLabel"
      note="Card colour, hairline edge, radius 10, padding 14/16; flat."
    >
      <div className="grid gap-3 sm:grid-cols-2">
        <Card>
          <SectionLabel action="Edit">Movie</SectionLabel>
          <p className="m-0 text-(length:--fs-13) text-text-2">
            A card holds one group of facts. Its label is the one section style: 11px, 600,
            capitals.
          </p>
        </Card>
        <Card compact>
          <SectionLabel>Compact</SectionLabel>
          <p className="m-0 text-(length:--fs-13) text-text-2">
            Tighter padding for narrow columns.
          </p>
        </Card>
      </div>
    </Group>
  );
}

const POSTERS: {
  title: string;
  year: string;
  quality: "2160p" | "1080p";
  stage: StageBadgeKind;
  note?: string;
}[] = [
  { title: "Arrival", year: "2016", quality: "2160p", stage: "scraped" },
  { title: "Blade Runner 2049", year: "2017", quality: "2160p", stage: "renamed" },
  { title: "The Batman", year: "2022", quality: "2160p", stage: "new" },
  { title: "Godzilla", year: "????", quality: "1080p", stage: "attention", note: "Not matched" },
];

function Tiles() {
  const [picked, setPicked] = useState<string | null>("Blade Runner 2049");
  return (
    <Group
      title="PosterTile and Skeleton"
      note="2:3 at radius 10 with the generated poster as the fallback. Hover lifts it; selected gets a gold outline and a check."
    >
      <div className="grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-4">
        {POSTERS.map((poster) => (
          <PosterTile
            key={poster.title}
            title={poster.title}
            year={poster.year}
            quality={poster.quality}
            stage={poster.stage}
            stageLabel={poster.note}
            note={poster.note}
            selected={picked === poster.title}
            onClick={() => setPicked((current) => (current === poster.title ? null : poster.title))}
          />
        ))}
      </div>
      <Row label="Skeletons: poster, row, line, tile (shown at once here; in the app after 150 ms)">
        <div className="flex w-full items-start gap-4">
          <div className="w-20 shrink-0">
            <Skeleton shape="poster" immediate />
          </div>
          <div className="flex min-w-0 flex-1 flex-col gap-2">
            <Skeleton shape="row" immediate />
            <Skeleton shape="line" width="70%" immediate />
            <Skeleton shape="line" width="45%" immediate />
            <Skeleton shape="tile" aspect="16 / 9" immediate className="max-w-48" />
          </div>
        </div>
      </Row>
    </Group>
  );
}

interface Title {
  id: number;
  title: string;
  year: number;
  quality: "2160p" | "1080p" | "720p";
  size: number;
  score: number;
  stage: StageBadgeKind;
}

const FILMS: Title[] = [
  { id: 1, title: "Alien", year: 1979, quality: "1080p", size: 14.2, score: 8.5, stage: "renamed" },
  {
    id: 2,
    title: "Arrival",
    year: 2016,
    quality: "2160p",
    size: 58.4,
    score: 7.9,
    stage: "scraped",
  },
  {
    id: 3,
    title: "Blade Runner 2049",
    year: 2017,
    quality: "2160p",
    size: 71.9,
    score: 8.0,
    stage: "renamed",
  },
  {
    id: 4,
    title: "Godzilla",
    year: 2014,
    quality: "1080p",
    size: 11.1,
    score: 6.4,
    stage: "attention",
  },
  { id: 5, title: "Heat", year: 1995, quality: "2160p", size: 63.0, score: 8.3, stage: "scraped" },
  { id: 6, title: "Solaris", year: 1972, quality: "720p", size: 6.8, score: 8.0, stage: "new" },
];

const COLUMNS: TableColumn<Title>[] = [
  {
    id: "stage",
    label: "Stage",
    width: 56,
    header: <span className="sr-only">Stage</span>,
    fixed: true,
    resizable: false,
    align: "center",
    cell: (film) => <StageBadge stage={film.stage} />,
  },
  { id: "title", label: "Title", width: 110, grow: true, sortable: true, cell: (f) => f.title },
  { id: "year", label: "Year", width: 56, sortable: true, cell: (f) => f.year },
  {
    id: "quality",
    label: "Quality",
    width: 88,
    cell: (f) => <QualityBadge quality={f.quality} />,
  },
  {
    id: "size",
    label: "Size",
    width: 80,
    align: "end",
    sortable: true,
    cell: (f) => `${f.size.toFixed(1)} GB`,
  },
  {
    id: "score",
    label: "Rating",
    width: 76,
    align: "end",
    cell: (f) => <ScoreBadge value={f.score.toFixed(1)} />,
  },
];

const TABLE_BAR_ACTIONS: SelectionBarAction[] = [
  { id: "rename", label: "Rename", icon: FolderPen, onSelect: () => {} },
  { id: "delete", label: "Delete", icon: Trash2, danger: true, onSelect: () => {} },
];

const BAR_MORE: SelectionBarAction[] = [
  { id: "nfo", label: "Rewrite NFO", icon: FolderPen, shortcut: "F4", onSelect: () => {} },
  { id: "open", label: "Open folder", icon: HardDrive, onSelect: () => {} },
];

function ListParts() {
  const [sort, setSort] = useState<TableSort>({ column: "title", direction: "asc" });
  const rows = useMemo(() => {
    const sorted = [...FILMS].sort((a, b) => {
      const key = sort.column === "year" ? "year" : sort.column === "size" ? "size" : "title";
      const result = a[key] < b[key] ? -1 : a[key] > b[key] ? 1 : 0;
      return sort.direction === "asc" ? result : -result;
    });
    return sorted;
  }, [sort]);
  const keys = useMemo(() => rows.map((row) => row.id), [rows]);
  const selection = useListSelection({ keys });
  const { select } = selection;
  // Start with two rows selected, so the selected row and the bar can be seen on load.
  useEffect(() => {
    select([2, 3]);
  }, [select]);
  const total = rows
    .filter((row) => selection.isSelected(row.id))
    .reduce((sum, row) => sum + row.size, 0);
  return (
    <Group
      title="Table, SelectionBar"
      note="Sticky 30px header, 34px rows, gold-tinted selected rows, a 2px gold edge on the current row. Click, Ctrl/Shift-click, arrows and Ctrl+A work."
    >
      <Stage className="h-[300px] overflow-hidden">
        <Table
          aria-label="Titles"
          rows={rows}
          rowKey={(row) => row.id}
          columns={COLUMNS}
          selection={selection}
          sort={sort}
          onSortChange={setSort}
          bottomRoom={72}
          className="h-full rounded-(--r-10)"
        />
        <SelectionBar
          count={selection.count}
          size={`${total.toFixed(1)} GB`}
          actions={TABLE_BAR_ACTIONS}
          more={BAR_MORE}
          onClear={selection.clear}
        />
      </Stage>
    </Group>
  );
}

let nextToast = 1;

interface ToastSpec {
  id: number;
  message: string;
  undo?: boolean;
  error?: boolean;
  count?: number;
}

function Toasts() {
  const toastLabel = useColumnLabel("Toast demo");
  const [toasts, setToasts] = useState<ToastSpec[]>([
    { id: 0, message: "Renamed 12 folders", undo: true },
    { id: -1, message: "Refreshed 7 episodes", count: 3 },
    { id: -2, message: "Could not scrape Heat", error: true },
  ]);
  const add = (spec: Omit<ToastSpec, "id">) =>
    setToasts((current) => [...current, { ...spec, id: nextToast++ }].slice(-3));
  const remove = (id: number) => setToasts((current) => current.filter((toast) => toast.id !== id));
  return (
    <Group
      title="Toast and ToastStack"
      note="Floating, bottom right, at most three, newest lowest; Undo shows its key; the time bar pauses under the pointer."
    >
      <Row label="Add one">
        <IconButton
          aria-label="Add an undo toast"
          icon={FolderPen}
          onClick={() => add({ message: "Renamed 4 folders", undo: true })}
        />
        <IconButton
          aria-label="Add a plain toast"
          icon={RefreshCw}
          onClick={() => add({ message: "Scraped 25 titles" })}
        />
        <IconButton
          aria-label="Add an error toast"
          icon={Trash2}
          onClick={() => add({ message: "Could not move Heat", error: true })}
        />
      </Row>
      <Stage className="h-[230px] overflow-hidden">
        <ToastStack position="absolute" label={toastLabel}>
          {toasts.map((toast) => (
            <Toast
              key={toast.id}
              message={toast.message}
              glyph={toast.error || toast.count ? undefined : FolderPen}
              count={toast.count}
              tone={toast.error ? "error" : "default"}
              onUndo={toast.undo ? () => remove(toast.id) : undefined}
              action={toast.error ? { label: "Show", onClick: () => remove(toast.id) } : undefined}
              onDismiss={() => remove(toast.id)}
              duration={null}
            />
          ))}
        </ToastStack>
      </Stage>
    </Group>
  );
}

/** The containers, status parts and list parts of the gallery. */
export function ContainerSections() {
  return (
    <>
      <Cards />
      <SidebarParts />
      <Bars />
      <Tiles />
      <ListParts />
      <Toasts />
    </>
  );
}
