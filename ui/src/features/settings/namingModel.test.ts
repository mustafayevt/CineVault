import { describe, expect, it } from "vitest";

import { displayTemplate } from "../rename/tokenLabels";
import {
  EPISODE_TOKENS,
  MOVIE_TOKENS,
  colonStyleOf,
  displayProblem,
  followColonStyle,
  insertToken,
  toDisplay,
  toEngine,
  withColonStyle,
} from "./namingModel";

describe("naming templates as the field shows them", () => {
  it("shows plain tokens by their friendly name", () => {
    expect(toDisplay("${title} (${year}) [${videoFormat} ${hdrformat}]", MOVIE_TOKENS)).toBe(
      "{Title} ({Year}) [{Resolution} {HDR}]",
    );
  });

  it("shows an optional token in the form the Rename page shows", () => {
    const engine = "${title}${ - ,edition,} (${year})";
    expect(toDisplay(engine, MOVIE_TOKENS)).toBe("{Title}{ - Edition?} ({Year})");
    expect(toEngine("{Title}{ - Edition?} ({Year})", MOVIE_TOKENS)).toBe(engine);
    expect(displayTemplate(engine)).toBe(toDisplay(engine, MOVIE_TOKENS));
    expect(displayProblem(toDisplay(engine, MOVIE_TOKENS), MOVIE_TOKENS)).toBeNull();
  });

  it("leaves expressions it has no name for as the engine writes them", () => {
    const engine = "${title}${ - ,edition,} (${year})${if tmdb > 0} {tmdb-${tmdb}}${end}";
    expect(toDisplay(engine, MOVIE_TOKENS)).toBe(
      "{Title}{ - Edition?} ({Year})${if tmdb > 0} {tmdb-{TMDB}}${end}",
    );
  });

  it("goes back to the engine's text without changing a character", () => {
    for (const engine of [
      "${title} (${year})",
      "${title}${ - ,edition,} (${year})",
      "${title} (${year})${if tmdb > 0} {tmdb-${tmdb}}${end}",
      "${title}; ${unknownToken}",
    ]) {
      expect(toEngine(toDisplay(engine, MOVIE_TOKENS), MOVIE_TOKENS)).toBe(engine);
    }
  });

  it("names the episode tokens for the TV template", () => {
    expect(toDisplay("${showTitle} - S${seasonNr2}E${episodeNr2} - ${title}", EPISODE_TOKENS)).toBe(
      "{Show} - S{Season00}E{Episode00} - {EpisodeTitle}",
    );
  });
});

describe("what makes a template invalid", () => {
  it("says a brace is not closed", () => {
    expect(displayProblem("{Title} ({Year", MOVIE_TOKENS)).toMatch(/not closed/);
    expect(displayProblem("{Title", MOVIE_TOKENS)).toContain("{Title");
  });

  it("says a closing brace has no opening one", () => {
    expect(displayProblem("{Title}}", MOVIE_TOKENS)).toMatch(/no opening brace/);
  });

  it("says a word in braces is no token", () => {
    expect(displayProblem("{Titel} ({Year})", MOVIE_TOKENS)).toMatch(/not a token/);
  });

  it("accepts nested braces of the engine's own expressions", () => {
    expect(displayProblem("{Title}${if tmdb > 0} {tmdb-{TMDB}}${end}", MOVIE_TOKENS)).toBeNull();
  });
});

describe("inserting a token", () => {
  it("puts it at the cursor and moves the cursor behind it", () => {
    expect(insertToken("{Title} ({Year})", 16, 16, "Resolution")).toEqual({
      text: "{Title} ({Year}){Resolution}",
      caret: 28,
    });
  });

  it("replaces the selection", () => {
    expect(insertToken("{Title} X", 8, 9, "Year")).toEqual({ text: "{Title} {Year}", caret: 14 });
  });
});

describe("the colon style", () => {
  it("writes a colon the three ways", () => {
    expect(withColonStyle("Dune: Part Two", "remove")).toBe("Dune Part Two");
    expect(withColonStyle("Dune: Part Two", "dash")).toBe("Dune - Part Two");
    expect(withColonStyle("Dune: Part Two", "lookalike")).toBe("Dune꞉ Part Two");
  });

  it("changes the title inside an example the backend wrote with a dash", () => {
    expect(followColonStyle("Dune - Part Two (2024)", "Dune: Part Two", "remove")).toBe(
      "Dune Part Two (2024)",
    );
    expect(followColonStyle("Heat (1995)", "Heat", "remove")).toBe("Heat (1995)");
  });

  it("changes an example the backend wrote in another style, and keeps one already in the style", () => {
    expect(followColonStyle("Dune꞉ Part Two (2024)", "Dune: Part Two", "dash")).toBe(
      "Dune - Part Two (2024)",
    );
    expect(followColonStyle("Dune Part Two (2024)", "Dune: Part Two", "lookalike")).toBe(
      "Dune꞉ Part Two (2024)",
    );
    expect(followColonStyle("Dune Part Two (2024)", "Dune: Part Two", "remove")).toBe(
      "Dune Part Two (2024)",
    );
  });

  it("starts as the renamer does without a choice: a dash", () => {
    expect(colonStyleOf(undefined)).toBe("dash");
    expect(colonStyleOf("garbage")).toBe("dash");
    expect(colonStyleOf("remove")).toBe("remove");
  });
});
