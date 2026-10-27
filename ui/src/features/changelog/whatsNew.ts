import { create } from "zustand";

import { useSettingsStore } from "../settings/settingsStore";
import { currentUpdatesBackend } from "../updates/updatesBackend";
import { BUNDLED_CHANGELOG } from "./bundledChangelog";
import { compareVersions, sectionOf } from "./changelogModel";
import type { ChangelogSection } from "./changelogModel";

/** The setting that holds the version whose "What's new" the person has been shown. */
export const LAST_SEEN_VERSION_KEY = "updates.last-seen-version";

/**
 * What to do at the start of a session. `show` is the section to present (the version that was
 * just installed), `record` says whether the current version should be stored as seen.
 *
 * - Same version as last time, or no version known (a plain browser): nothing.
 * - A newer version than last time, or an upgrade from a build that did not remember (no stored
 *   version): show its section when the changelog has one.
 * - An older version (a downgrade) or a version without a section: store it quietly.
 *
 * A fresh install never gets here without a stored version: the first-run setup stores it.
 */
export function decideWhatsNew(input: {
  lastSeen: unknown;
  current: string;
  sections: readonly ChangelogSection[];
}): { show: ChangelogSection | null; record: boolean } {
  const { lastSeen, current, sections } = input;
  if (current === "" || lastSeen === current) return { show: null, record: false };
  const previous = typeof lastSeen === "string" ? lastSeen : null;
  const order = previous === null ? -1 : compareVersions(previous, current);
  const section = sectionOf(sections, current);
  const newer = order === null || order < 0;
  return { show: newer && section !== null && section.body !== "" ? section : null, record: true };
}

/** Stores the running version as seen (after the first-run setup, so a new install shows nothing). */
export async function markCurrentVersionSeen(): Promise<void> {
  try {
    const { current_version: version } = await currentUpdatesBackend().status();
    if (version !== "") useSettingsStore.getState().set(LAST_SEEN_VERSION_KEY, version);
  } catch {
    // Without a known version there is nothing to remember.
  }
}

interface WhatsNewState {
  /** The section being shown, or `null`. */
  section: ChangelogSection | null;
  show: (section: ChangelogSection) => void;
  dismiss: () => void;
}

export const useWhatsNewStore = create<WhatsNewState>((set) => ({
  section: null,
  show: (section) => {
    set({ section });
  },
  dismiss: () => {
    set({ section: null });
  },
}));

/** Applies the decision once settings and the version are known; used by the shell. */
export function startWhatsNew(current: string): void {
  const { values } = useSettingsStore.getState();
  const decision = decideWhatsNew({
    lastSeen: values[LAST_SEEN_VERSION_KEY],
    current,
    sections: BUNDLED_CHANGELOG,
  });
  if (decision.record) useSettingsStore.getState().set(LAST_SEEN_VERSION_KEY, current);
  if (decision.show !== null) useWhatsNewStore.getState().show(decision.show);
}
