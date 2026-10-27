import { describe, expect, it } from "vitest";

import { diffWords, tokenize } from "./wordDiff";
import type { DiffSegment } from "./wordDiff";

/** The changed runs of one side, in order. */
const changed = (segments: DiffSegment[]): string[] =>
  segments.filter((segment) => segment.changed).map((segment) => segment.text);

/** The words of a name that are inside a changed run: every one must be whole. */
function wordsIn(segments: DiffSegment[], changedOnly: boolean): string[] {
  return segments
    .filter((segment) => segment.changed === changedOnly)
    .flatMap((segment) => segment.text.match(/[\p{L}\p{N}]+(?:['’][\p{L}\p{N}]+)*/gu) ?? []);
}

describe("the word tokenizer", () => {
  it("keeps a word with its apostrophe and splits the rest into separators and brackets", () => {
    expect(tokenize("Director's Cut (1986)").map((token) => `${token.kind}:${token.text}`)).toEqual(
      [
        "word:Director's",
        "separator: ",
        "word:Cut",
        "separator: ",
        "open:(",
        "word:1986",
        "close:)",
      ],
    );
  });

  it("treats a bracket that never closes as a plain character", () => {
    const kinds = tokenize("Heat (1995").map((token) => token.kind);
    expect(kinds).not.toContain("open");
    expect(kinds).toContain("other");
  });
});

describe("the word diff of a rename", () => {
  it("keeps whole words and shows the separators and the brackets that changed", () => {
    const diff = diffWords("A.Nightmare.on.Elm.Street.1984", "A Nightmare on Elm Street (1984)");
    // Every word of the title stays, none is split; the year is struck and added as a whole.
    expect(wordsIn(diff.before, false)).toEqual(["A", "Nightmare", "on", "Elm", "Street"]);
    expect(wordsIn(diff.after, false)).toEqual(["A", "Nightmare", "on", "Elm", "Street"]);
    expect(wordsIn(diff.before, true)).toEqual(["1984"]);
    expect(wordsIn(diff.after, true)).toEqual(["1984"]);
    // The brackets are added with their word, not left behind.
    expect(changed(diff.after).join("")).toContain("(1984)");
    expect(changed(diff.before).join("")).toContain("1984");
    // The dots that became spaces are marked on both sides.
    expect(changed(diff.before).some((text) => text.includes("."))).toBe(true);
    expect(changed(diff.after).some((text) => text.includes(" "))).toBe(true);
    // Nothing is lost: the segments say the two names again.
    expect(diff.before.map((s) => s.text).join("")).toBe("A.Nightmare.on.Elm.Street.1984");
    expect(diff.after.map((s) => s.text).join("")).toBe("A Nightmare on Elm Street (1984)");
  });

  it("strikes a removed edition with its brackets and keeps the rest", () => {
    const diff = diffWords("Aliens (1986) [Director's Cut]", "Aliens (1986)");
    expect(wordsIn(diff.before, true)).toEqual(["Director's", "Cut"]);
    expect(changed(diff.before).join("")).toContain("[Director's Cut]");
    expect(wordsIn(diff.before, false)).toEqual(["Aliens", "1986"]);
    expect(changed(diff.after)).toEqual([]);
  });

  it("marks only the words that differ inside a bracket and keeps the brackets", () => {
    const diff = diffWords("Heat (1995) [1080p]", "Heat (1995) [1080p HDR]");
    expect(wordsIn(diff.after, true)).toEqual(["HDR"]);
    expect(changed(diff.before)).toEqual([]);
    // The closing bracket still closes the same group, so it is not marked.
    expect(changed(diff.after).join("")).not.toContain("]");
  });

  it("compares words without regard to case, and leaves identical names unmarked", () => {
    expect(changed(diffWords("heat (1995)", "Heat (1995)").before)).toEqual([]);
    const same = diffWords("Heat (1995)", "Heat (1995)");
    expect(same.before).toEqual([{ text: "Heat (1995)", changed: false }]);
    expect(same.after).toEqual([{ text: "Heat (1995)", changed: false }]);
  });

  it("never cuts a word, whatever the names are", () => {
    const names = [
      ["Les.Miserables.2012.REPACK", "Les Misérables (2012)"],
      ["Se7en.1995.1080p.BluRay.x264-GRP", "Se7en (1995) [1080p]"],
      ["Dune - Part Two (2024)", "Dune Part Two (2024) {tmdb-693134}"],
    ] as const;
    for (const [from, to] of names) {
      const diff = diffWords(from, to);
      for (const [side, text] of [
        [diff.before, from],
        [diff.after, to],
      ] as const) {
        const rebuilt = side.map((segment) => segment.text).join("");
        expect(rebuilt).toBe(text);
        // A word is in one run only: a run boundary never falls inside a word.
        for (let index = 0; index + 1 < side.length; index += 1) {
          const left = side[index]?.text.at(-1) ?? "";
          const right = side[index + 1]?.text.at(0) ?? "";
          expect(/[\p{L}\p{N}]/u.test(left) && /[\p{L}\p{N}]/u.test(right)).toBe(false);
        }
      }
    }
  });
});
