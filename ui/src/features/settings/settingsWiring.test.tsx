/** Settings that did something only on paper are wired, hidden or removed. */

import { beforeEach, describe, expect, it } from "vitest";

import { displayTitle } from "../library/displayTitle";
import { SETTINGS, settingDef } from "./schema";
import { useSettingsStore } from "./settingsStore";

beforeEach(() => {
  useSettingsStore.setState({ values: {}, status: "ready" });
});

describe("settings that read nothing are removed or hidden", () => {
  it("no longer offers the renamer templates or the server refresh", () => {
    for (const id of [
      "renamer.movie-folder-template",
      "renamer.movie-file-template",
      "integrations.server-refresh",
    ]) {
      expect(settingDef(id), id).toBeUndefined();
      expect(
        SETTINGS.some((def) => def.id === id),
        id,
      ).toBe(false);
    }
  });

  it("keeps the settings that are wired", () => {
    for (const id of [
      "advanced.log-level",
      "movies.collections.auto-group",
      "providers.prefer-original-title",
      "tv.episode-order",
      "tv.specials.show",
    ]) {
      expect(
        SETTINGS.some((def) => def.id === id),
        id,
      ).toBe(true);
    }
  });
});

describe("the original title preference", () => {
  it("picks the original title only when asked and when there is one", () => {
    expect(displayTitle("Spirited Away", "千と千尋の神隠し", false)).toBe("Spirited Away");
    expect(displayTitle("Spirited Away", "千と千尋の神隠し", true)).toBe("千と千尋の神隠し");
    expect(displayTitle("Spirited Away", "", true)).toBe("Spirited Away");
    expect(displayTitle("Spirited Away", undefined, true)).toBe("Spirited Away");
    expect(displayTitle("Spirited Away", "  ", true)).toBe("Spirited Away");
  });
});
