import { describe, expect, it } from "vitest";

import { fieldByName } from "./fields";
import { chipToTerm, extractChips, formatQuery, parseQuery, removeTerm } from "./queryParser";
import type { SearchChip } from "./queryParser";

const flag = (field: SearchChip["field"], negated = false): SearchChip => ({
  field,
  negated,
  values: [],
  min: null,
  max: null,
});

const range = (
  field: SearchChip["field"],
  min: number | null,
  max: number | null,
  negated = false,
): SearchChip => ({ field, negated, values: [], min, max });

const list = (field: SearchChip["field"], values: string[], negated = false): SearchChip => ({
  field,
  negated,
  values,
  min: null,
  max: null,
});

describe("the scenario query", () => {
  it("turns hdr:dv year:2010..2019 -new into three chips and no text", () => {
    const parsed = parseQuery("hdr:dv year:2010..2019 -new");
    expect(parsed.chips).toEqual([
      list("hdr", ["Dolby Vision"]),
      range("year", 2010, 2019),
      flag("new", true),
    ]);
    expect(parsed.text).toBe("");
    expect(parsed.rejected).toEqual([]);
  });

  it("keeps title words as text beside the chips", () => {
    const parsed = parseQuery("heat hdr:dv the 1995");
    expect(parsed.chips).toEqual([list("hdr", ["Dolby Vision"])]);
    expect(parsed.text).toBe("heat the 1995");
  });
});

describe("numbers and ranges", () => {
  it.each([
    ["year:2015", 2015, 2015],
    ["year:2010..2019", 2010, 2019],
    ["year:2010..", 2010, null],
    ["year:..2000", null, 2000],
    ["year:>2010", 2011, null],
    ["year:>=2010", 2010, null],
    ["year:<2000", null, 1999],
    ["year:<=2000", null, 2000],
    ["runtime:>120", 121, null],
    ["rating:7.5..", 7.5, null],
    ["rating:>7", 7.1, null],
    ["rating:<8", null, 7.9],
    ["top250:..50", null, 50],
  ])("reads %s", (term, min, max) => {
    const [chip] = parseQuery(term).chips;
    expect(chip).toMatchObject({ min, max, negated: false, values: [] });
  });

  it.each([
    "year:abc",
    "year:2019..2010",
    "year:201",
    "year:2010.5",
    "year:",
    "year:..",
    "rating:11",
    "top250:300",
    "runtime:>=",
  ])("rejects %s without searching for it as a title", (term) => {
    const parsed = parseQuery(term);
    expect(parsed.chips).toEqual([]);
    expect(parsed.text).toBe("");
    expect(parsed.rejected.map((t) => t.raw)).toEqual([term]);
  });

  it("negates a range with a leading minus", () => {
    expect(parseQuery("-year:2010..2019").chips).toEqual([range("year", 2010, 2019, true)]);
  });
});

describe("lists", () => {
  it("reads quoted values with spaces", () => {
    expect(parseQuery('actor:"mel gibson"').chips).toEqual([list("actor", ["mel gibson"])]);
  });

  it("splits commas outside quotes and drops repeats", () => {
    expect(parseQuery('genre:drama,"sci-fi, noir",drama').chips).toEqual([
      list("genre", ["drama", "sci-fi, noir"]),
    ]);
  });

  it("negates a list", () => {
    expect(parseQuery("-hdr:hdr10").chips).toEqual([list("hdr", ["HDR10"], true)]);
  });

  it.each([
    ["hdr:dv", "Dolby Vision"],
    ["hdr:DolbyVision", "Dolby Vision"],
    ["hdr:hdr10+", "HDR10+"],
    ["hdr:HDR10plus", "HDR10+"],
    ["hdr:hlg", "HLG"],
    ["resolution:4k", "2160p"],
    ["res:1080", "1080p"],
    ["resolution:SD", "480p"],
  ])("normalises %s to %s", (term, value) => {
    expect(parseQuery(term).chips[0]?.values).toEqual([value]);
  });

  it("rejects a value the field cannot have", () => {
    const parsed = parseQuery("hdr:xyz resolution:huge");
    expect(parsed.chips).toEqual([]);
    expect(parsed.rejected.map((t) => t.name)).toEqual(["hdr", "resolution"]);
  });

  it("accepts a field's aliases in any case", () => {
    expect(parseQuery("Actors:Heston").chips[0]?.field).toBe("actor");
    expect(parseQuery("cast:Heston").chips[0]?.field).toBe("actor");
    expect(parseQuery("SUBS:fr").chips[0]?.field).toBe("subtitle_language");
    expect(fieldByName("nothing")).toBeUndefined();
  });
});

describe("yes/no terms", () => {
  it("needs the minus (or is:) so a title word is not taken", () => {
    expect(parseQuery("new york").chips).toEqual([]);
    expect(parseQuery("new york").text).toBe("new york");
    expect(parseQuery("-new").chips).toEqual([flag("new", true)]);
    expect(parseQuery("is:new").chips).toEqual([flag("new")]);
    expect(parseQuery("-is:new").chips).toEqual([flag("new", true)]);
  });

  it("reads the lifecycle flags the pipeline counts link to", () => {
    expect(parseQuery("is:review").chips).toEqual([flag("review")]);
    expect(parseQuery("is:notmatched").chips).toEqual([flag("not_matched")]);
    expect(parseQuery("is:renamepending").chips).toEqual([flag("rename_pending")]);
    expect(parseQuery("is:missing").chips).toEqual([flag("missing")]);
    expect(parseQuery("-is:unmatched").chips).toEqual([flag("not_matched", true)]);
  });

  it("reads new:yes and new:no", () => {
    expect(parseQuery("new:yes").chips).toEqual([flag("new")]);
    expect(parseQuery("new:no").chips).toEqual([flag("new", true)]);
    expect(parseQuery("-new:no").chips).toEqual([flag("new")]);
    expect(parseQuery("new:maybe").rejected).toHaveLength(1);
  });

  it("rejects is: with something that is not a yes/no field", () => {
    expect(parseQuery("is:cool").rejected.map((t) => t.name)).toEqual(["is"]);
  });
});

describe("what stays plain text", () => {
  it("leaves unknown fields, a lone minus and unmatched minus words alone", () => {
    const parsed = parseQuery("foo:bar - -heat spider-man");
    expect(parsed.chips).toEqual([]);
    expect(parsed.text).toBe("foo:bar - -heat spider-man");
  });

  it("removes the quotes of a quoted phrase", () => {
    expect(parseQuery('"the matrix" reloaded').text).toBe("the matrix reloaded");
  });

  it("is empty for empty input", () => {
    expect(parseQuery("")).toMatchObject({ chips: [], text: "", rejected: [], openQuote: false });
    expect(parseQuery("   ").terms).toEqual([]);
  });
});

describe("quotes", () => {
  it("does not end a term at a space inside quotes", () => {
    const parsed = parseQuery('director:"michael mann" heat');
    expect(parsed.terms.map((t) => t.raw)).toEqual(['director:"michael mann"', "heat"]);
  });

  it("reads an unclosed quote as a value in progress and says so", () => {
    const parsed = parseQuery('actor:"mel gib');
    expect(parsed.openQuote).toBe(true);
    expect(parsed.chips).toEqual([list("actor", ["mel gib"])]);
  });
});

describe("terms keep their place in the input", () => {
  it("reports where each term starts and ends", () => {
    const input = "a  hdr:dv  -new";
    const parsed = parseQuery(input);
    for (const term of parsed.terms) expect(input.slice(term.start, term.end)).toBe(term.raw);
    expect(parsed.terms.map((t) => t.kind)).toEqual(["free", "chip", "chip"]);
  });

  it("removes one term and tidies the spaces", () => {
    const input = "heat hdr:dv  -new";
    const dv = parseQuery(input).terms[1];
    expect(removeTerm(input, dv?.start ?? 0, dv?.end ?? 0)).toBe("heat -new");
    expect(removeTerm("hdr:dv", 0, 6)).toBe("");
    expect(removeTerm("heat hdr:dv", 5, 11)).toBe("heat");
  });

  it("extracts chips and keeps the other terms as typed", () => {
    expect(extractChips('heat hdr:dv year:abc "the end"')).toEqual({
      chips: [list("hdr", ["Dolby Vision"])],
      rest: 'heat year:abc "the end"',
    });
  });
});

describe("writing a query back", () => {
  const chips: SearchChip[] = [
    list("hdr", ["Dolby Vision"]),
    range("year", 2010, 2019),
    flag("new", true),
    flag("new"),
    range("year", 2015, 2015, true),
    range("runtime", 100, null),
    range("rating", null, 7.5),
    list("actor", ["mel gibson", "Sean Connery"]),
    list("genre", ["Sci-Fi"], true),
  ];

  it("writes terms that parse back to the same chips", () => {
    for (const chip of chips) {
      expect(parseQuery(chipToTerm(chip)).chips).toEqual([chip]);
    }
  });

  it("joins chips and text", () => {
    expect(formatQuery([list("hdr", ["Dolby Vision"]), flag("new", true)], " heat ")).toBe(
      'hdr:"Dolby Vision" -new heat',
    );
    expect(parseQuery(formatQuery(chips, "heat")).chips).toEqual(chips);
  });
});
