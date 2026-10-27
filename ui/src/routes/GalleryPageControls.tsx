import {
  ArrowRightLeft,
  Download,
  Film,
  FolderPen,
  Grid2x2,
  List,
  Play,
  Rows3,
  Search,
  Trash2,
} from "lucide-react";
import { useState } from "react";

import { Button, IconButton } from "../components/Button";
import type { ButtonSize, ButtonVariant } from "../components/Button";
import { Checkbox } from "../components/Checkbox";
import { Chip } from "../components/Chip";
import { Field } from "../components/Field";
import { Kbd } from "../components/Kbd";
import { QualityBadge } from "../components/QualityBadge";
import { ScoreBadge } from "../components/ScoreBadge";
import { SearchField } from "../components/SearchField";
import { SegmentedControl } from "../components/SegmentedControl";
import { Select } from "../components/Select";
import { StageBadge } from "../components/StageBadge";
import { Switch } from "../components/Switch";
import { Tag } from "../components/Tag";
import { cx } from "../lib/cx";
import { Group, Row, Stage } from "./GalleryPageParts";

/**
 * The look a button has under the pointer, pressed and with the keyboard on it, drawn by forcing
 * the same rungs the real states use (`!` wins over the rest-state class), so every state can be
 * seen at once without a pointer.
 */
const FORCED: Record<"hover" | "pressed" | "focus", Record<ButtonVariant, string>> = {
  hover: {
    default: "bg-surface-active!",
    primary: "bg-gold-hover! border-gold-hover!",
    ghost: "bg-surface-active! text-text!",
    danger: "bg-surface-active!",
  },
  pressed: {
    default: "bg-surface-pressed!",
    primary: "brightness-95",
    ghost: "bg-surface-pressed! text-text!",
    danger: "bg-surface-pressed!",
  },
  focus: {
    default: "outline-2 outline-offset-2 outline-text",
    primary: "outline-2 outline-offset-2 outline-text",
    ghost: "outline-2 outline-offset-2 outline-text",
    danger: "outline-2 outline-offset-2 outline-text",
  },
};

const VARIANTS: { variant: ButtonVariant; label: string }[] = [
  { variant: "primary", label: "Primary" },
  { variant: "default", label: "Default" },
  { variant: "ghost", label: "Ghost" },
  { variant: "danger", label: "Danger" },
];

const SIZES: { size: ButtonSize; label: string }[] = [
  { size: "sm", label: "Small 24" },
  { size: "md", label: "Regular 28" },
  { size: "lg", label: "Large 32" },
];

function Buttons() {
  return (
    <Group
      title="Button and IconButton"
      note="One family at three heights. One primary (gold) per screen; Large is for dialogs and commit steps."
    >
      {SIZES.map(({ size, label }) => (
        <Row key={size} label={label}>
          {VARIANTS.map(({ variant, label: text }) => (
            <Button key={variant} variant={variant} size={size}>
              {text}
            </Button>
          ))}
          <Button size={size} icon={Download}>
            With icon
          </Button>
        </Row>
      ))}
      {(["rest", "hover", "pressed", "focus", "disabled"] as const).map((state) => (
        <Row key={state} label={`State: ${state}`}>
          {VARIANTS.map(({ variant, label }) => (
            <Button
              key={variant}
              variant={variant}
              disabled={state === "disabled"}
              className={
                state === "rest" || state === "disabled" ? undefined : FORCED[state][variant]
              }
            >
              {label}
            </Button>
          ))}
        </Row>
      ))}
      <Row label="Icon, shortcut and reason">
        <Button variant="primary" icon={Play} shortcut="Enter">
          Play
        </Button>
        <Button icon={FolderPen} shortcut="F2">
          Rename
        </Button>
        <Button variant="danger" icon={Trash2}>
          Delete 3 titles
        </Button>
        <Button disabledReason="Needs a match first" enabledBy="Scrape…">
          Rename
        </Button>
      </Row>
      <Row label="Icon button 28 (tooltip is the name)">
        <IconButton aria-label="Search library" icon={Search} />
        <IconButton aria-label="Move titles" icon={ArrowRightLeft} />
        <IconButton aria-label="Pressed toggle" icon={Rows3} pressed />
        <IconButton aria-label="Disabled action" icon={Trash2} disabled />
        <IconButton aria-label="Small action" icon={Search} size="sm" />
        <IconButton aria-label="Large action" icon={Search} size="lg" />
      </Row>
    </Group>
  );
}

function Inputs() {
  const [query, setQuery] = useState("heat");
  const [sort, setSort] = useState("title");
  const [checked, setChecked] = useState(true);
  const [mixed, setMixed] = useState(false);
  const [on, setOn] = useState(true);
  return (
    <Group
      title="Field, SearchField, Select, Checkbox, Switch"
      note="30px fields on the card colour; the well colour inside floating surfaces. Focus turns the border gold, with no halo."
    >
      <Row label="Field: rest, filled, focus, error, disabled" className="items-start">
        <Field aria-label="Rest" placeholder="Title" className="w-40" />
        <Field aria-label="Filled" defaultValue="{title} ({year})" mono className="w-40" />
        <Field
          aria-label="Focus"
          defaultValue="Focused"
          boxClassName="border-gold!"
          className="w-40"
        />
        <Field
          aria-label="Error"
          defaultValue="{titel}"
          mono
          error="Unknown token {titel}. Use {title}."
          className="w-52"
        />
        <Field aria-label="Disabled" defaultValue="Locked" disabled className="w-40" />
      </Row>
      <Row label="Field with label, description, icon and shortcut" className="items-start">
        <Field
          label="Library folder"
          description="Movies live here."
          defaultValue="D:\Movies"
          mono
          className="w-52"
        />
        <Field
          aria-label="Quick find"
          icon={Film}
          shortcut="Ctrl K"
          placeholder="Find"
          className="w-52"
        />
      </Row>
      <Row label="Search field 34px">
        <SearchField
          aria-label="Search the library"
          placeholder="Search"
          className="w-60"
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onClear={() => setQuery("")}
        />
        <SearchField
          aria-label="Empty search"
          placeholder="Search"
          className="w-40"
          value=""
          readOnly
        />
      </Row>
      <Row label="On a floating surface (well colour)">
        <div className="flex items-center gap-2 rounded-(--r-10) border border-solid border-line-edge bg-surface-floating p-3">
          <Field aria-label="In a menu" floating placeholder="Filter" className="w-40" />
          <Select
            aria-label="Sort in a menu"
            floating
            className="w-36"
            value={sort}
            onValueChange={setSort}
            options={[
              { value: "title", label: "Title" },
              { value: "year", label: "Year" },
            ]}
          />
        </div>
      </Row>
      <Row label="Select: rest and disabled">
        <Select
          aria-label="Sort by"
          className="w-40"
          value={sort}
          onValueChange={setSort}
          options={[
            { value: "title", label: "Title" },
            { value: "year", label: "Year" },
            { value: "size", label: "Size" },
          ]}
        />
        <Select
          aria-label="Sort by, disabled"
          className="w-40"
          disabled
          value="title"
          onValueChange={() => {}}
          options={[{ value: "title", label: "Title" }]}
        />
      </Row>
      <Row label="Checkbox 14px: off, on, mixed, disabled">
        <Checkbox aria-label="Off" checked={false} onCheckedChange={() => {}} />
        <Checkbox aria-label="On" checked={checked} onCheckedChange={setChecked} />
        <Checkbox aria-label="Mixed" checked={mixed} indeterminate onCheckedChange={setMixed} />
        <Checkbox aria-label="Disabled off" checked={false} disabled onCheckedChange={() => {}} />
        <Checkbox aria-label="Disabled on" checked disabled onCheckedChange={() => {}} />
        <Checkbox checked={checked} onCheckedChange={setChecked}>
          With a label
        </Checkbox>
      </Row>
      <Row label="Switch: on, off, disabled">
        <Switch aria-label="On" checked={on} onCheckedChange={setOn} />
        <Switch aria-label="Off" checked={false} onCheckedChange={() => {}} />
        <Switch aria-label="Disabled on" checked disabled onCheckedChange={() => {}} />
        <Switch aria-label="Disabled off" checked={false} disabled onCheckedChange={() => {}} />
      </Row>
    </Group>
  );
}

function Selection() {
  const [view, setView] = useState("grid");
  const [kind, setKind] = useState("movies");
  const [quality, setQuality] = useState("all");
  const [chips, setChips] = useState({ scraped: true, drive: false });
  return (
    <Group
      title="SegmentedControl and Chip"
      note="Track on the card colour, 24px segments; chips are 24px pills that take a gold tint when on."
    >
      <Row label="Segmented: text, text with counts, icons">
        <SegmentedControl
          aria-label="Library kind"
          value={kind}
          onChange={setKind}
          options={[
            { value: "movies", label: "Movies" },
            { value: "tv", label: "TV shows" },
            { value: "collections", label: "Collections" },
          ]}
        />
        <SegmentedControl
          aria-label="Resolution"
          value={quality}
          onChange={setQuality}
          options={[
            { value: "all", label: "All", count: 128 },
            { value: "2160", label: "2160p", count: 41 },
            { value: "1080", label: "1080p", count: 87 },
          ]}
        />
        <SegmentedControl
          aria-label="View"
          value={view}
          onChange={setView}
          options={[
            { value: "grid", icon: Grid2x2, ariaLabel: "Poster grid" },
            { value: "list", icon: List, ariaLabel: "List" },
            { value: "table", icon: Rows3, ariaLabel: "Table" },
          ]}
        />
      </Row>
      <Row label="Chip: off, on, with count, icon, menu, warn, disabled">
        <Chip
          active={chips.scraped}
          count={86}
          onClick={() => setChips((c) => ({ ...c, scraped: !c.scraped }))}
        >
          Scraped
        </Chip>
        <Chip
          active={chips.drive}
          icon={Film}
          onClick={() => setChips((c) => ({ ...c, drive: !c.drive }))}
        >
          D: Media
        </Chip>
        <Chip count={12}>Not matched</Chip>
        <Chip tone="warn" count={3}>
          Problems
        </Chip>
        <Chip disabled>Disabled</Chip>
      </Row>
    </Group>
  );
}

function Badges() {
  return (
    <Group
      title="Tag, QualityBadge, ScoreBadge, StageBadge, Kbd"
      note="18px, 11/600, radius 4. One quality badge and one score badge everywhere; stage is never colour alone."
    >
      <Row label="Tag">
        <Tag>Not watched</Tag>
        <Tag variant="ok">Complete</Tag>
        <Tag variant="warn">Low space</Tag>
      </Row>
      <Row label="Quality">
        <QualityBadge quality="2160p" />
        <QualityBadge quality="2160p" hdr="Dolby Vision" />
        <QualityBadge quality="1080p" hdr />
        <QualityBadge quality="720p" />
        <QualityBadge quality="SD" />
      </Row>
      <Row label="Score (700, tabular)">
        <ScoreBadge value={8.4} source="IMDb" />
        <ScoreBadge value="92%" source="Tomatometer" />
        <ScoreBadge value={79} source="Metacritic" />
      </Row>
      <Row label="Stage: New, Scraped, Renamed, needs attention">
        <span className="inline-flex items-center gap-1.5 text-(length:--fs-12) text-text-2">
          <StageBadge stage="new" /> New
        </span>
        <span className="inline-flex items-center gap-1.5 text-(length:--fs-12) text-text-2">
          <StageBadge stage="scraped" /> Scraped
        </span>
        <span className="inline-flex items-center gap-1.5 text-(length:--fs-12) text-text-2">
          <StageBadge stage="renamed" /> Renamed
        </span>
        <span className="inline-flex items-center gap-1.5 text-(length:--fs-12) text-text-2">
          <StageBadge stage="attention" label="Not matched" /> Not matched
        </span>
      </Row>
      <Row label="Key caps">
        <Kbd>Del</Kbd>
        <Kbd>Shift Del</Kbd>
        <Kbd>Ctrl K</Kbd>
        <Stage
          tone="card"
          className={cx("flex items-center gap-2 px-2 py-1 text-(length:--fs-12) text-text-2")}
        >
          Undo <Kbd>Ctrl Z</Kbd>
        </Stage>
      </Row>
    </Group>
  );
}

/** The buttons, inputs, selectors and small labels of the gallery. */
export function ControlSections() {
  return (
    <>
      <Buttons />
      <Inputs />
      <Selection />
      <Badges />
    </>
  );
}
