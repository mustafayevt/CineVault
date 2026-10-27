import {
  ArrowLeftRight,
  Eye,
  EyeOff,
  FileText,
  FolderOpen,
  Image,
  PenLine,
  Play,
  ScanSearch,
  SquarePen,
  Trash2,
  Unlink,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

import { t } from "../../i18n/catalog";
import type { MessageKey } from "../../i18n/catalog";

/** Every action on a title, in the words of the three verbs (Scan, Scrape, Rename) and the rest. */
export type TitleActionId =
  | "open"
  | "play"
  | "open-folder"
  | "rename"
  | "scrape"
  | "artwork"
  | "move"
  | "rewrite-nfo"
  | "edit"
  | "remove"
  | "ignore"
  | "delete"
  | "delete-permanently";

export interface TitleActionDef {
  id: TitleActionId;
  icon: LucideIcon;
  /** The short name on buttons (the selection bar, the title page header). */
  short: MessageKey;
  /** The full name in menus (right-click menu, More): the same words with their counts. */
  full: MessageKey;
  /** The key that does it, shown in menus. */
  hint?: string;
  /** A destructive action: the danger colour. */
  danger?: boolean;
  /** The action works on one title only (Open, Play, Edit details). */
  single?: boolean;
}

/**
 * The one list of title actions. The selection bar, the right-click menu, the More
 * menu and the title pages' header all read their icons, names, key hints and order from here, so
 * a name or an order cannot drift between them.
 */
export const TITLE_ACTIONS: Readonly<Record<TitleActionId, TitleActionDef>> = {
  open: {
    id: "open",
    icon: Eye,
    short: "library.act.open",
    full: "library.act.open",
    hint: "Enter",
    single: true,
  },
  play: {
    id: "play",
    icon: Play,
    short: "title.action.play",
    full: "title.action.play",
    single: true,
  },
  "open-folder": {
    id: "open-folder",
    icon: FolderOpen,
    short: "title.action.openFolder",
    full: "title.action.openFolder",
    single: true,
  },
  rename: {
    id: "rename",
    icon: PenLine,
    short: "title.action.rename",
    full: "library.act.rename",
    hint: "F2",
  },
  scrape: {
    id: "scrape",
    icon: ScanSearch,
    short: "title.action.scrape",
    full: "library.act.scrape",
  },
  artwork: {
    id: "artwork",
    icon: Image,
    short: "library.act.artwork.short",
    full: "library.act.artwork",
  },
  move: {
    id: "move",
    icon: ArrowLeftRight,
    short: "title.action.move",
    full: "library.act.move",
  },
  "rewrite-nfo": {
    id: "rewrite-nfo",
    icon: FileText,
    short: "title.menu.rewriteNfo",
    full: "title.menu.rewriteNfo",
  },
  edit: {
    id: "edit",
    icon: SquarePen,
    short: "title.menu.edit",
    full: "title.menu.edit",
    single: true,
  },
  remove: {
    id: "remove",
    icon: Unlink,
    short: "title.menu.remove",
    full: "title.menu.remove",
  },
  ignore: {
    id: "ignore",
    icon: EyeOff,
    short: "title.menu.ignore",
    full: "title.menu.ignore",
  },
  delete: {
    id: "delete",
    icon: Trash2,
    short: "title.action.delete",
    full: "library.act.delete",
    hint: "Del",
    danger: true,
  },
  "delete-permanently": {
    id: "delete-permanently",
    icon: Trash2,
    short: "library.act.deletePermanently",
    full: "library.act.deletePermanently",
    hint: "Shift Del",
    danger: true,
  },
};

/** The buttons of the selection bar, left to right. */
export const BAR_ORDER: readonly TitleActionId[] = [
  "rename",
  "scrape",
  "artwork",
  "move",
  "delete",
];

/** Behind the bar's More button. */
export const MORE_ORDER: readonly TitleActionId[] = [
  "rewrite-nfo",
  "edit",
  "remove",
  "ignore",
  "delete-permanently",
];

/** The right-click menu: groups divided by a separator. Every action of the bar and More is here. */
export const MENU_GROUPS: readonly (readonly TitleActionId[])[] = [
  ["open", "play", "open-folder"],
  ["rename", "scrape", "artwork", "move", "rewrite-nfo", "edit", "remove", "ignore"],
  ["delete", "delete-permanently"],
];

/** The buttons of a title page header; Delete follows after a 12px gap. */
export const HEADER_ORDER: readonly TitleActionId[] = [
  "play",
  "scrape",
  "rename",
  "move",
  "open-folder",
  "delete",
];

/** The action's name on a button. */
export function shortLabel(id: TitleActionId): string {
  return t(TITLE_ACTIONS[id].short);
}

/** The action's name in a menu, with the count when several titles are selected. */
export function fullLabel(id: TitleActionId, count: number): string {
  return t(TITLE_ACTIONS[id].full, { count });
}
