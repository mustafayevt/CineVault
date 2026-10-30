import { useSearch } from "@tanstack/react-router";
import { useCallback, useEffect, useRef, useState } from "react";
import type { ComponentType } from "react";

import { t } from "../../i18n/catalog";
import { isServer } from "../../lib/transport";
import { AppearanceSection } from "./AppearanceSection";
import { DeletedFilesSection } from "./DeletedFilesSection";
import { FoldersSection } from "./FoldersSection";
import { NamingSection } from "./NamingSection";
import { ProvidersSection } from "./ProvidersSection";
import { ServerSection } from "./ServerSection";
import { SettingsNav } from "./SettingsNav";
import { TorrentSourcesSection } from "./TorrentSourcesSection";
import { UpdatesSection } from "./UpdatesSection";

interface SectionDef {
  id: string;
  label: () => string;
  Body: ComponentType;
}

/** The sections in page order; the index follows this list. */
const BASE_SECTIONS: readonly SectionDef[] = [
  { id: "providers", label: () => t("settingsPage.providers.title"), Body: ProvidersSection },
  { id: "naming", label: () => t("settingsPage.naming.title"), Body: NamingSection },
  { id: "folders", label: () => t("settingsPage.folders.title"), Body: FoldersSection },
  {
    id: "deleted-files",
    label: () => t("settingsPage.deleted.title"),
    Body: DeletedFilesSection,
  },
  { id: "torrents", label: () => t("settingsPage.torrents.title"), Body: TorrentSourcesSection },
  { id: "appearance", label: () => t("settingsPage.appearance.title"), Body: AppearanceSection },
  { id: "updates", label: () => t("settingsPage.updates.title"), Body: UpdatesSection },
];

/** The Server section exists only when the page is served by the browser server. */
const SECTIONS: readonly SectionDef[] =
  isServer()
    ? [...BASE_SECTIONS, { id: "server", label: () => t("settingsPage.server.title"), Body: ServerSection }]
    : BASE_SECTIONS;

/** Older addresses (`?section=library`) and the ids of the old groups, by their new section. */
const ALIASES: Readonly<Record<string, string>> = {
  library: "folders",
  general: "appearance",
};

/** The section a `?section=` value names, or `null`. */
function sectionOf(value: string | undefined): string | null {
  if (value === undefined) return null;
  const id = ALIASES[value] ?? value;
  return SECTIONS.some((section) => section.id === id) ? id : null;
}

/** The distance from the scroller's top at which a section counts as the one being read. */
const READING_LINE = 48;
/** How long the index ignores the scroll after a click, while the page scrolls to the section. */
const PIN_MS = 700;

/**
 * The Settings page (`/settings`): one scrolling page of sections with a flat
 * index at its left. Changes apply at once; there is no Save. `?section=<id>` opens the page at
 * that section, as the Rename page's "Edit in Settings" does.
 */
export function SettingsPage() {
  const { section: wanted } = useSearch({ strict: false }) as { section?: string };
  const [active, setActive] = useState<string>(sectionOf(wanted) ?? SECTIONS[0]?.id ?? "");
  const scroller = useRef<HTMLDivElement>(null);
  const pinnedUntil = useRef(0);

  const scrollTo = useCallback((id: string, instant: boolean) => {
    const target = scroller.current?.querySelector<HTMLElement>(`[data-settings-section="${id}"]`);
    if (target === null || target === undefined) return;
    pinnedUntil.current = Date.now() + PIN_MS;
    setActive(id);
    const reduced =
      typeof window.matchMedia === "function" &&
      window.matchMedia("(prefers-reduced-motion: reduce)").matches;
    target.scrollIntoView?.({ block: "start", behavior: instant || reduced ? "auto" : "smooth" });
  }, []);

  useEffect(() => {
    const id = sectionOf(wanted);
    if (id !== null) scrollTo(id, true);
  }, [wanted, scrollTo]);

  const onScroll = () => {
    const el = scroller.current;
    if (el === null || Date.now() < pinnedUntil.current) return;
    const top = el.getBoundingClientRect().top;
    let current = SECTIONS[0]?.id ?? "";
    for (const section of SECTIONS) {
      const node = el.querySelector<HTMLElement>(`[data-settings-section="${section.id}"]`);
      if (node !== null && node.getBoundingClientRect().top - top <= READING_LINE) {
        current = section.id;
      }
    }
    // The last sections are short: at the bottom of the page the last one is the one being read.
    if (el.scrollTop > 0 && el.scrollTop + el.clientHeight >= el.scrollHeight - 2) {
      current = SECTIONS[SECTIONS.length - 1]?.id ?? current;
    }
    setActive(current);
  };

  return (
    <section aria-label={t("settingsPage.region")} className="relative flex h-full min-h-0">
      <h1 className="sr-only">{t("settingsPage.region")}</h1>
      <SettingsNav
        entries={SECTIONS.map((section) => ({ id: section.id, label: section.label() }))}
        active={active}
        onSelect={(id) => {
          scrollTo(id, false);
        }}
      />
      <div ref={scroller} onScroll={onScroll} className="min-w-0 flex-1 overflow-y-auto">
        <div className="flex max-w-[860px] flex-col gap-[26px] px-6 py-6">
          {SECTIONS.map(({ id, Body }) => (
            <Body key={id} />
          ))}
        </div>
      </div>
    </section>
  );
}
