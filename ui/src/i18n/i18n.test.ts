import { describe, expect, it } from "vitest";

import { SETTINGS } from "../features/settings/schema";
import { EN_AREAS, allMessages, hasMessage, t, tList } from "./catalog";
import type { MessageKey } from "./catalog";
import { createFormatters, formatBytes } from "./format";
import { LOCALE_SYSTEM, defaultSizeBase, resolveLocale, useLocaleStore } from "./locale";

describe("t", () => {
  it("inserts parameters and formats numbers for the selected locale", () => {
    expect(t("move.titles", { count: 3 })).toBe("3 titles");
    expect(t("move.titles", { count: 1234 })).toBe("1,234 titles");
    useLocaleStore.setState({ preference: "de-DE" });
    expect(t("move.titles", { count: 1234 })).toBe("1.234 titles");
  });

  it("chooses a plural branch and puts the formatted number in place of #", () => {
    expect(t("move.titles", { count: 1 })).toBe("1 title");
    expect(t("move.titles", { count: 0 })).toBe("0 titles");
    expect(t("move.titles", { count: 2500 })).toBe("2,500 titles");
  });

  it("keeps plural rules of the catalogue language whatever the regional format is", () => {
    useLocaleStore.setState({ preference: "ru-RU" });
    expect(t("move.titles", { count: 1 })).toBe("1 title");
    expect(t("move.titles", { count: 2 })).toBe("2 titles");
  });

  it("leaves a missing parameter visible instead of throwing", () => {
    expect(t("history.toast.left", { title: "Heat" })).toBe("Heat: {reason}");
  });

  it("splits a keyword message into a list", () => {
    expect(tList("settings.library.skip-nomedia.keywords")).toEqual([
      "exclude",
      "ignore",
      "marker",
      "nomedia",
      "hidden",
    ]);
  });

  it("recognises catalogue keys", () => {
    expect(hasMessage("chrome.back")).toBe(true);
    expect(hasMessage("nav.nowhere")).toBe(false);
  });
});

describe("the English catalogue", () => {
  it("has no message that renders an unfilled placeholder or stray brace", () => {
    for (const [key, template] of Object.entries(allMessages())) {
      const names = [...template.matchAll(/\{(\w+)[,}]/g)].map((match) => match[1] ?? "");
      const params = Object.fromEntries(names.map((name) => [name, name === "count" ? 2 : "x"]));
      const text = t(key as MessageKey, params);
      expect(text, key).not.toMatch(/[{}]/);
    }
  });

  it("uses the three verbs: no Identify, Enrich, Rescrape (but the hero button) or Search & scrape in user-facing text", () => {
    // Search keywords are hidden synonyms (people still type "identify"), so they are exempt.
    const retired = /\b(identif\w*|enrich\w*|rescrap\w*)\b|search\s*(&|&amp;|and)\s*scrape/i;
    for (const [key, template] of Object.entries(allMessages())) {
      if (key.endsWith(".keywords")) continue;
      expect(template, key).not.toMatch(retired);
    }
  });

  it("sends people only to Settings pages that exist", () => {
    // The groups and sub-pages of Settings as the page names them. Kodi's and the providers'
    // own menus ("In Kodi, turn on Settings > Services", "Open Settings, API" on TMDB) are not
    // CineVault pages, so their messages are exempt.
    const pages = [
      "General",
      "Library and folders",
      "Features",
      "Automation",
      "Scraping and providers",
      "Artwork, subtitles and trailers",
      "NFO files",
      "Advanced",
      "Media centres",
      "Trakt sync",
      "HTTP API",
      "Export",
      "Scripts",
      "Backups",
      "External tools",
      "Keyboard shortcuts",
      "Torrent sources",
    ];
    for (const [key, template] of Object.entries(allMessages())) {
      if (key.startsWith("providers.info.")) continue;
      for (const match of template.matchAll(/Settings, /g)) {
        const rest = template.slice((match.index ?? 0) + match[0].length);
        expect(
          pages.some((page) => rest.startsWith(page)),
          `${key}: Settings, ${rest}`,
        ).toBe(true);
      }
      if (!/Kodi/.test(template)) expect(template, key).not.toMatch(/Settings > /);
      expect(template, key).not.toMatch(/Providers page/);
    }
  });

  it("labels the actions with the glossary verbs", () => {
    expect(t("library.scan")).toBe("Scan folders now");
    expect(t("title.action.scrape")).toBe("Scrape");
  });

  it("is not empty anywhere", () => {
    for (const [key, template] of Object.entries(allMessages())) {
      expect(template.trim(), key).not.toBe("");
    }
  });

  it("covers every setting and option", () => {
    for (const def of SETTINGS) {
      expect(def.title, def.id).not.toMatch(/^settings\./);
      expect(def.description, def.id).not.toMatch(/^settings\./);
      expect(def.keywords.length, def.id).toBeGreaterThan(0);
      if (def.control.type === "choice") {
        for (const option of def.control.options) {
          expect(option.label, `${def.id}/${option.value}`).not.toMatch(/^settings\./);
        }
      }
    }
  });
});

describe("resolveLocale", () => {
  it("uses the stated locale, and falls back to the system's for a bad tag", () => {
    expect(resolveLocale("de-DE")).toBe("de-DE");
    expect(resolveLocale("not a locale")).toBe(resolveLocale(LOCALE_SYSTEM));
    expect(resolveLocale(LOCALE_SYSTEM)).toMatch(/^[a-z]{2,3}(-|$)/i);
  });
});

describe("formatBytes", () => {
  const GIB = 1024 ** 3;

  it("counts in thousands for base 10 and in 1024s for base 2", () => {
    expect(formatBytes(0, "en-US", "base10")).toBe("0 B");
    expect(formatBytes(999, "en-US", "base10")).toBe("999 B");
    expect(formatBytes(1000, "en-US", "base10")).toBe("1 kB");
    expect(formatBytes(1024, "en-US", "base10")).toBe("1.02 kB");
    expect(formatBytes(1024, "en-US", "base2")).toBe("1 KiB");
    expect(formatBytes(1023, "en-US", "base2")).toBe("1,023 B");
  });

  it("shows the same file as GB or GiB", () => {
    const bytes = 4.7 * GIB;
    expect(formatBytes(bytes, "en-US", "base10")).toBe("5.05 GB");
    expect(formatBytes(bytes, "en-US", "base2")).toBe("4.7 GiB");
    expect(formatBytes(50 * 1000 ** 3, "en-US", "base10")).toBe("50 GB");
    expect(formatBytes(50 * GIB, "en-US", "base2")).toBe("50 GiB");
    expect(formatBytes(1500 * 1000 ** 3, "en-US", "base10")).toBe("1.5 TB");
    expect(formatBytes(1500 * GIB, "en-US", "base2")).toBe("1.46 TiB");
  });

  it("uses fewer decimals as the number grows", () => {
    expect(formatBytes(1.234 * 1000 ** 2, "en-US", "base10")).toBe("1.23 MB");
    expect(formatBytes(12.34 * 1000 ** 2, "en-US", "base10")).toBe("12.3 MB");
    expect(formatBytes(123.4 * 1000 ** 2, "en-US", "base10")).toBe("123 MB");
  });

  it("carries a value that rounds up to the next unit", () => {
    expect(formatBytes(999_999, "en-US", "base10")).toBe("1 MB");
    expect(formatBytes(1_048_575, "en-US", "base2")).toBe("1 MiB");
  });

  it("follows the locale's digits and separators", () => {
    expect(formatBytes(4.7 * 1000 ** 3, "de-DE", "base10")).toBe("4,7 GB");
    expect(formatBytes(1234 * 1000 ** 3, "de-DE", "base10")).toBe("1,23 TB");
    expect(formatBytes(1234 * GIB, "fr-FR", "base2")).toBe("1,21 TiB");
  });

  it("does not invent a unit past petabytes and reports invalid sizes", () => {
    expect(formatBytes(2000 * 1000 ** 5, "en-US", "base10")).toBe("2,000 PB");
    expect(formatBytes(-1, "en-US", "base10")).toBe("Unknown size");
    expect(formatBytes(Number.NaN, "en-US", "base2")).toBe("Unknown size");
    expect(formatBytes(Number.POSITIVE_INFINITY, "en-US", "base2")).toBe("Unknown size");
  });
});

describe("createFormatters", () => {
  const moment = Date.UTC(2026, 9, 20, 16, 5);

  it("formats numbers, percentages and lists for the locale", () => {
    const en = createFormatters("en-US", "base10");
    const de = createFormatters("de-DE", "base10");
    expect(en.number(1234567.5)).toBe("1,234,567.5");
    expect(de.number(1234567.5)).toBe("1.234.567,5");
    expect(en.percent(0.25)).toBe("25%");
    expect(en.list(["a", "b", "c"])).toBe("a, b, c");
    expect(en.andList(["a", "b"])).toBe("a and b");
  });

  it("formats dates and times for the locale", () => {
    const options = { timeZone: "UTC" };
    const us = createFormatters("en-US", "base10");
    const gb = createFormatters("en-GB", "base10");
    const de = createFormatters("de-DE", "base10");
    expect(us.date(moment, options)).toBe("Oct 20, 2026");
    expect(gb.date(moment, options)).toBe("20 Oct 2026");
    expect(de.date(moment, options)).toBe("20.10.2026");
    expect(us.time(moment, options)).toMatch(/^4:05\sPM$/);
    expect(gb.time(moment, options)).toBe("16:05");
    expect(de.time(moment, options)).toBe("16:05");
    expect(gb.dateTime(moment, options)).toContain("20 Oct 2026");
    expect(gb.dateTime(moment, options)).toContain("16:05");
  });

  it("accepts a Date, epoch milliseconds or an ISO string, and returns nothing for a bad date", () => {
    const gb = createFormatters("en-GB", "base10");
    const options = { timeZone: "UTC" };
    expect(gb.date(new Date(moment), options)).toBe("20 Oct 2026");
    expect(gb.date("2026-10-20T16:05:00Z", options)).toBe("20 Oct 2026");
    expect(gb.date("not a date")).toBe("");
    expect(gb.time(Number.NaN)).toBe("");
  });

  it("formats sizes in its own size base", () => {
    expect(createFormatters("en-US", "base10").size(1024 ** 3)).toBe("1.07 GB");
    expect(createFormatters("en-US", "base2").size(1024 ** 3)).toBe("1 GiB");
  });
});

describe("the default size base", () => {
  it("counts in 1024s on Windows, as Explorer does, and in 1000s elsewhere", () => {
    expect(defaultSizeBase("Mozilla/5.0 (Windows NT 10.0; Win64; x64) Edg/130")).toBe("base2");
    expect(defaultSizeBase("Mozilla/5.0 (Macintosh; Intel Mac OS X 14_0) AppleWebKit")).toBe(
      "base10",
    );
    expect(defaultSizeBase("")).toBe("base10");
  });
});

describe("the area files of the catalogue", () => {
  it("has one file per page area", () => {
    expect(Object.keys(EN_AREAS).sort()).toEqual(
      [
        "artwork",
        "common",
        "delete",
        "drives",
        "history",
        "library",
        "movie",
        "onboarding",
        "rename",
        "scrape",
        "settings",
        "shell",
        "show",
        "torrents",
      ].sort(),
    );
  });

  it("keeps every key in exactly one area, and the catalogue is their union", () => {
    const owners = new Map<string, string[]>();
    for (const [area, messages] of Object.entries(EN_AREAS)) {
      for (const key of Object.keys(messages)) owners.set(key, [...(owners.get(key) ?? []), area]);
    }
    const shared = [...owners].filter(([, areas]) => areas.length > 1);
    expect(shared).toEqual([]);
    expect([...owners.keys()].sort()).toEqual(Object.keys(allMessages()).sort());
  });
});
