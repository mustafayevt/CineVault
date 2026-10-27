import { describe, expect, it } from "vitest";

import { selectionOf, validateRenameSearch } from "./renameSearch";
import {
  classifyToken,
  dollarQuery,
  insertToken,
  matchTokens,
  parseTemplate,
  removeToken,
  serializeTemplate,
  setLiteral,
  tokenFor,
} from "./template";
import { chipText, displayTemplate, tokenLabel } from "./tokenLabels";

/** The templates of the three presets, as the Rust side ships them. */
const PRESET_TEMPLATES = [
  "${title} (${year})",
  "${title}${ - ,edition,} (${year})",
  "${title} (${year}) [${videoFormat}] [${mediaSource}] [${hdrformat}] [${audioCodec} ${audioChannelsDot}]",
  "${title} (${year})${if tmdb > 0} {tmdb-${tmdb}}${end}",
  "${title} (${year})${if edition} {edition-${edition}}${end}",
  "${showTitle} (${showYear})",
  "Season ${seasonNr2}",
  "${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}",
  "${showTitle} (${showYear})${if showTmdb > 0} {tmdb-${showTmdb}}${end}",
];

describe("template text and chips", () => {
  it("reads simple tokens, optional segments and advanced expressions", () => {
    const model = parseTemplate("${title}${ - ,edition,} (${year})${if tmdb > 0} x${end}");
    expect(model.tokens.map((t) => [t.kind, t.name])).toEqual([
      ["token", "title"],
      ["optional", "edition"],
      ["token", "year"],
      ["advanced", ""],
      ["advanced", ""],
    ]);
    expect(model.tokens[1]).toMatchObject({ prefix: " - ", suffix: "" });
    expect(model.literals).toEqual(["", "", " (", ")", " x", ""]);
  });

  it("gives every chip a plain label", () => {
    expect(chipText(classifyToken("${title}"))).toBe("Title");
    expect(chipText(classifyToken("${videoFormat}"))).toBe("Resolution");
    expect(chipText(classifyToken("${ - ,edition,}"))).toBe("- Edition?");
    expect(chipText(classifyToken("${if tmdb > 0}"))).toBe("if tmdb > 0");
    expect(chipText(classifyToken("${somethingNew}"))).toBe("Something new");
  });

  it("keeps a token with a renderer, and a renderer with commas, as one token", () => {
    expect(classifyToken("${title;first}")).toMatchObject({ kind: "token", name: "title" });
    expect(classifyToken("${title;replace(a,b)}")).toMatchObject({ kind: "token", name: "title" });
    expect(classifyToken("${movie.title}")).toMatchObject({ kind: "token", name: "movie.title" });
  });

  it("treats keywords as advanced, never as token names", () => {
    expect(classifyToken("${end}").kind).toBe("advanced");
    expect(classifyToken("${else}").kind).toBe("advanced");
  });

  it("leaves an unclosed ${ as plain text", () => {
    const model = parseTemplate("${title} ${year");
    expect(model.tokens).toHaveLength(1);
    expect(model.literals).toEqual(["", " ${year"]);
  });

  it("turns text into chips and back without changing a character", () => {
    const tricky = [
      "",
      "plain",
      "$",
      "${",
      "${}",
      "{}",
      "${title",
      "$${title}",
      "${a}${b}${c}",
      "${if x}{${y}}${end}",
      "${ {edition-,edition,}}",
      "é ${title} ü (${year}) 日本語",
      "${title} {nested {braces}} ${year}",
      " leading and trailing ",
    ];
    for (const text of [...PRESET_TEMPLATES, ...tricky]) {
      expect(serializeTemplate(parseTemplate(text))).toBe(text);
    }
  });

  it("round-trips pseudo-random text made of the template's special characters", () => {
    const alphabet = [
      "$",
      "{",
      "}",
      ",",
      ";",
      "(",
      ")",
      " ",
      "a",
      "title",
      "if x",
      "end",
      "\\",
      "é",
    ];
    let seed = 12345;
    const next = () => {
      seed = (seed * 1103515245 + 12345) % 2147483648;
      return seed;
    };
    for (let i = 0; i < 500; i += 1) {
      let text = "";
      const length = next() % 16;
      for (let j = 0; j < length; j += 1) text += alphabet[next() % alphabet.length] ?? "";
      expect(serializeTemplate(parseTemplate(text))).toBe(text);
    }
  });

  it("edits the model: insert a chip, remove a chip, type a whole token into text", () => {
    let model = parseTemplate("${title} (${year})");
    model = insertToken(model, 1, 2, tokenFor("edition"));
    expect(serializeTemplate(model)).toBe("${title} (${edition}${year})");
    model = removeToken(model, 1);
    expect(serializeTemplate(model)).toBe("${title} (${year})");
    const typed = setLiteral(parseTemplate("${title}"), 1, " - ${year}");
    expect(typed.tokens.map((t) => t.name)).toEqual(["title", "year"]);
    expect(serializeTemplate(typed)).toBe("${title} - ${year}");
  });

  it("keeps the literals around a removed chip together", () => {
    const model = removeToken(parseTemplate("a${x}b${y}c"), 0);
    expect(model.literals).toEqual(["ab", "c"]);
    expect(model.tokens.map((t) => t.name)).toEqual(["y"]);
  });
});

describe("the $ autocomplete query", () => {
  it("finds the $ just before the caret and what was typed after it", () => {
    expect(dollarQuery("$", 1)).toEqual({ start: 0, query: "" });
    expect(dollarQuery("abc $ye", 7)).toEqual({ start: 4, query: "ye" });
    expect(dollarQuery("$year more", 5)).toEqual({ start: 0, query: "year" });
  });

  it("ignores a finished ${ and text without a $", () => {
    expect(dollarQuery("${ye", 4)).toBeNull();
    expect(dollarQuery("plain", 5)).toBeNull();
    expect(dollarQuery("$ye ", 4)).toBeNull();
  });

  it("lists tokens that start with the query before those that contain it", () => {
    const tokens = [
      { name: "originalTitle", label: "Original title" },
      { name: "title", label: "Title" },
      { name: "titleSortable", label: "Sortable title" },
      { name: "year", label: "Year" },
    ];
    expect(matchTokens(tokens, "ti").map((t) => t.name)).toEqual([
      "title",
      "titleSortable",
      "originalTitle",
    ]);
    expect(matchTokens(tokens, "").length).toBe(4);
    expect(matchTokens(tokens, "zzz")).toEqual([]);
    expect(matchTokens(tokens, "ye", 1).map((t) => t.name)).toEqual(["year"]);
  });
});

describe("the rename route search", () => {
  it("reads ids, a lone number included, and drops the rest", () => {
    expect(validateRenameSearch({ ids: "1,2,3" })).toEqual({ ids: "1,2,3" });
    expect(validateRenameSearch({ ids: 7 })).toEqual({ ids: "7" });
    expect(validateRenameSearch({ ids: "  " })).toEqual({});
    expect(validateRenameSearch({ ids: { a: 1 } })).toEqual({});
    expect(validateRenameSearch({})).toEqual({});
  });

  it("splits ids without blanks or repeats", () => {
    expect(selectionOf({ ids: "4, 5,,5,6" })).toEqual(["4", "5", "6"]);
    expect(selectionOf({})).toBeNull();
    expect(selectionOf({ ids: "," })).toBeNull();
  });
});

describe("token labels", () => {
  it("names every token of the renamer for people, never by its internal name", () => {
    for (const [name, label] of [
      ["originalBasename", "Original file name, no extension"],
      ["sorttitle", "Sort title"],
      ["releaseDate", "Release date"],
      ["audioCodecList", "Audio codecs of every track"],
      ["audioChannelsAsString", "Audio channels, all tracks"],
      ["videoFormat", "Resolution"],
    ] as const) {
      expect(tokenLabel(name)).toBe(label);
    }
  });

  it("spells out a token the catalogue does not name yet", () => {
    expect(tokenLabel("someNewToken2")).toBe("Some new token 2");
  });
});

describe("the one-line template display", () => {
  it("names tokens in braces and keeps other expressions as written", () => {
    expect(displayTemplate("${title} (${year})")).toBe("{Title} ({Year})");
    expect(displayTemplate("${title}${ - ,edition,} (${year})")).toBe(
      "{Title}{ - Edition?} ({Year})",
    );
    expect(displayTemplate("${title}${if edition} x${end}")).toBe("{Title}${if edition} x${end}");
  });
});
