import type { LucideIcon } from "lucide-react";
import {
  ArrowRightLeft,
  ChevronDown,
  Download,
  FolderPen,
  HardDrive,
  RefreshCw,
  Search,
  Trash2,
} from "lucide-react";
import { useState } from "react";
import type { ReactNode } from "react";

import { Button, IconButton } from "../components/Button";
import { ContextMenu } from "../components/ContextMenu";
import { Dialog } from "../components/Dialog";
import { Field } from "../components/Field";
import {
  floatingSurface,
  FloatingContainerContext,
  menuItemClass,
  menuSurface,
} from "../components/floating";
import { Icon } from "../components/Icon";
import { Kbd } from "../components/Kbd";
import { Menu, MenuLabel, MenuSeparator } from "../components/Menu";
import { MenuItem } from "../components/MenuItem";
import { Popover } from "../components/Popover";
import { PosterTile } from "../components/PosterTile";
import { ProgressBar } from "../components/ProgressBar";
import { SelectionBar } from "../components/SelectionBar";
import { Skeleton } from "../components/Skeleton";
import { SpaceBar } from "../components/SpaceBar";
import { Toast, ToastStack } from "../components/Toast";
import { Tooltip } from "../components/Tooltip";
import { binName } from "../features/shell/binName";
import { detectPlatform } from "../features/shell/platform";
import { cx } from "../lib/cx";
import { useColumnLabel } from "./galleryTheme";
import { Group, Row, Stage } from "./GalleryPageParts";

/** An item of the static menu specimen: the same class as a live `MenuItem`. */
function StaticItem({
  icon,
  shortcut,
  danger,
  disabled,
  highlighted,
  children,
}: {
  icon: LucideIcon;
  shortcut?: string;
  danger?: boolean;
  disabled?: boolean;
  highlighted?: boolean;
  children: ReactNode;
}) {
  return (
    <div
      className={cx(
        menuItemClass,
        danger && "text-danger",
        disabled && "cursor-not-allowed text-text-disabled",
        highlighted && "bg-surface-active",
      )}
    >
      <Icon icon={icon} />
      {children}
      {shortcut !== undefined && <Kbd className="ml-auto">{shortcut}</Kbd>}
    </div>
  );
}

/** Keeps a floating surface from taking focus when it opens on load, so the page does not jump. */
const keepFocus = (event: Event) => event.preventDefault();

function MenuItems({ highlight = false }: { highlight?: boolean }) {
  const bin = binName(detectPlatform());
  return (
    <>
      <MenuItem icon={FolderPen} shortcut="F2">
        Rename
      </MenuItem>
      <MenuItem
        icon={RefreshCw}
        shortcut="F5"
        className={highlight ? "bg-surface-active!" : undefined}
      >
        Scrape
      </MenuItem>
      <MenuItem icon={ArrowRightLeft} disabledReason="Needs a second drive" enabledBy="Add a drive">
        Move to…
      </MenuItem>
      <MenuSeparator />
      <MenuLabel>Danger</MenuLabel>
      <MenuItem icon={Trash2} danger shortcut="Del">
        Delete to {bin}
      </MenuItem>
    </>
  );
}

/** Menu, Popover, Tooltip and the right-click menu, open so their surfaces can be seen. */
export function FloatingSections() {
  const bin = binName(detectPlatform());
  return (
    <Group
      title="Menu, ContextMenu, Popover, Tooltip, Dialog"
      note="One floating recipe: the floating surface, a 1px edge, the floating shadow; it fades in and grows from 0.98 at its anchor. Esc closes the innermost layer."
    >
      <div className="grid gap-4">
        <Stage className="flex min-h-[210px] items-start p-3">
          {/* The surface of an open menu, drawn from the same classes as the live one (the live
              menu opens from "Open menu" below, so nothing floats over the page while reading). */}
          <div aria-hidden="true" className={cx(menuSurface, "animate-none!")}>
            <StaticItem icon={FolderPen} shortcut="F2">
              Rename
            </StaticItem>
            <StaticItem icon={RefreshCw} shortcut="F5" highlighted>
              Scrape
            </StaticItem>
            <StaticItem icon={ArrowRightLeft} disabled>
              Move to…
            </StaticItem>
            <div className="mx-1.5 my-1 h-px bg-line-section" />
            <div className="type-section-label flex h-7 items-center px-2.5">Danger</div>
            <StaticItem icon={Trash2} danger shortcut="Del">
              Delete to {bin}
            </StaticItem>
          </div>
        </Stage>
        <Stage className="flex min-h-[130px] items-start p-3">
          <div
            aria-hidden="true"
            className={cx(floatingSurface, "animate-none! w-60 rounded-(--r-10) p-3")}
          >
            <div className="flex flex-col gap-2">
              <span className="text-(length:--fs-13) font-(weight:--fw-600)">Filter by drive</span>
              <Field aria-label="Drive name" floating placeholder="Name" tabIndex={-1} />
              <div className="flex justify-end gap-2">
                <Button size="sm" variant="ghost" tabIndex={-1}>
                  Reset
                </Button>
                <Button size="sm" variant="primary" tabIndex={-1}>
                  Apply
                </Button>
              </div>
            </div>
          </div>
        </Stage>
      </div>
      <Row label="Live menu and popover (click; Esc closes, focus returns)">
        <Menu
          aria-label="Title actions"
          trigger={<Button trailingIcon={ChevronDown}>Actions</Button>}
        >
          <MenuItems />
        </Menu>
        <Popover
          aria-label="Filter by drive"
          className="w-60"
          trigger={
            <Button icon={HardDrive} trailingIcon={ChevronDown}>
              Drive
            </Button>
          }
        >
          <span className="text-(length:--fs-13)">Filter by drive</span>
        </Popover>
      </Row>
      <Row label="Tooltip: the surface, and live on hover or keyboard focus (500 ms)">
        <div
          role="presentation"
          className={cx(
            floatingSurface,
            "animate-none! rounded-(--r-6) px-2 py-1 text-(length:--fs-12) leading-snug select-none",
          )}
        >
          Rename 3 titles
        </div>
        <Tooltip content="Search library">
          <IconButton aria-label="Search library (live tooltip)" icon={Search} />
        </Tooltip>
      </Row>
      <Row label="Right-click menu: right-click the area, or focus it and press Shift+F10">
        <ContextMenu aria-label="Row actions" menu={<MenuItems />}>
          <button
            type="button"
            aria-label="Right-click area"
            className="flex h-16 w-full max-w-sm items-center justify-center rounded-(--r-10) border border-dashed border-line-edge text-(length:--fs-13) text-text-muted"
          >
            Right-click here
          </button>
        </ContextMenu>
      </Row>
      <DialogDemo />
    </Group>
  );
}

function DialogDemo() {
  // The dialog is drawn inside this stage (a transformed box holds `fixed` children), so it can be
  // opened here without covering the gallery.
  const [stage, setStage] = useState<HTMLDivElement | null>(null);
  const bin = binName(detectPlatform());
  return (
    <Row label="Dialog: radius 14, scrim, focus trapped (opens inside this frame)">
      <FloatingContainerContext value={stage}>
        <div
          ref={setStage}
          style={{ transform: "translateZ(0)" }}
          className="relative flex h-[260px] w-full items-start overflow-hidden rounded-(--r-10) border border-solid border-line-section bg-surface-window p-3"
        >
          <Dialog
            trigger={<Button>Delete titles…</Button>}
            title={`Move 3 titles to the ${bin}?`}
            description={`Their folders go to the ${bin}. History can undo it.`}
            className="w-[360px]"
            footer={
              <>
                <Button size="lg" variant="ghost">
                  Cancel
                </Button>
                <Button size="lg" variant="danger" icon={Trash2}>
                  Move to {bin}
                </Button>
              </>
            }
          >
            <Field label="Type the folder name" floating placeholder="Heat (1995)" />
          </Dialog>
        </div>
      </FloatingContainerContext>
    </Row>
  );
}

/** Buttons that replay the motion of each primitive. */
export function MotionSections() {
  const toastLabel = useColumnLabel("Motion toast demo");
  const [bar, setBar] = useState(3);
  const [expanded, setExpanded] = useState(false);
  const [percent, setPercent] = useState(30);
  const [page, setPage] = useState(0);
  const [replay, setReplay] = useState(0);
  const [toasts, setToasts] = useState<number[]>([]);
  return (
    <Group
      title="Motion"
      note="Each button replays one primitive. Menus, popovers and tooltips fade and grow from 0.98 in 140 ms; the selection bar and toasts slide up in 180 ms; bars glide; nothing exceeds 250 ms. Reduced motion keeps only opacity."
    >
      <Row label="Menu and popover (open them)">
        <Menu
          aria-label="Motion menu"
          trigger={<Button>Open menu</Button>}
          onOpenAutoFocus={keepFocus}
        >
          <MenuItems />
        </Menu>
        <Popover
          aria-label="Motion popover"
          className="w-52"
          trigger={<Button>Open popover</Button>}
        >
          <span className="text-(length:--fs-13)">Grows from its trigger.</span>
        </Popover>
        <Tooltip content="Appears after 500 ms of rest">
          <Button variant="ghost" icon={Download}>
            Hover for a tooltip
          </Button>
        </Tooltip>
      </Row>
      <Row label="Selection bar and toast">
        <Button onClick={() => setBar((count) => (count === 0 ? 3 : 0))}>
          Toggle selection bar
        </Button>
        <Button onClick={() => setToasts((list) => [...list, Date.now()].slice(-3))}>
          Add toast
        </Button>
        <Stage className="relative h-28 w-full overflow-hidden">
          <SelectionBar
            count={bar}
            size="186.2 GB"
            actions={[{ id: "rename", label: "Rename", icon: FolderPen, onSelect: () => {} }]}
            onClear={() => setBar(0)}
          />
          <ToastStack position="absolute" label={toastLabel}>
            {toasts.map((id) => (
              <Toast
                key={id}
                message="Renamed 4 folders"
                glyph={FolderPen}
                onUndo={() => {}}
                onDismiss={() => setToasts((list) => list.filter((item) => item !== id))}
                duration={null}
              />
            ))}
          </ToastStack>
        </Stage>
      </Row>
      <Row label="Expand (height by grid rows)">
        <Button onClick={() => setExpanded((value) => !value)} aria-expanded={expanded}>
          {expanded ? "Collapse" : "Expand"}
        </Button>
        <div className="cv-expand w-full" data-open={expanded ? "true" : "false"}>
          <div className="rounded-(--r-6) bg-surface-card px-3 py-2 text-(length:--fs-13) text-text-2">
            Three more files: movie.mkv, movie.srt and poster.jpg. Closed, none of it can take
            focus.
          </div>
        </div>
      </Row>
      <Row label="Progress and space bars glide to the new value">
        <Button onClick={() => setPercent((value) => (value >= 100 ? 0 : value + 25))}>
          Advance 25%
        </Button>
        <Button variant="ghost" onClick={() => setPercent(30)}>
          Reset
        </Button>
        <div className="grid w-full max-w-md gap-3">
          <ProgressBar label="Replay progress" percent={percent} />
          <SpaceBar used={percent / 100} label="Replay space" />
        </div>
      </Row>
      <Row label="Page crossfade and hover lift">
        <Button onClick={() => setPage((value) => value + 1)}>Change page</Button>
        <div
          key={page}
          className="cv-page rounded-(--r-6) bg-surface-card px-3 py-2 text-(length:--fs-13)"
        >
          Page {page + 1}
        </div>
        <div className="w-20">
          <PosterTile title="Hover me" stage="scraped" />
        </div>
      </Row>
      <Row label="Skeleton waits 150 ms before it shows">
        <Button onClick={() => setReplay((value) => value + 1)}>Replay</Button>
        <div key={replay} className="w-16">
          <Skeleton shape="poster" />
        </div>
        <div key={`line-${replay}`} className="w-40">
          <Skeleton shape="line" width="100%" />
        </div>
      </Row>
    </Group>
  );
}
