import type {
  FlagKindDto,
  LifecycleCountsDto,
  LifecycleDto,
  LifecycleFlagDto,
  LifecycleLineDto,
  StageDto,
} from "../../bindings";

/**
 * Fixture lifecycles for a plain browser (no desktop backend) and tests. They follow the same
 * rules as the backend (`cv_app::lifecycle`): a title the search found nothing for is New and
 * "Not matched"; an uncertain match is New and "Needs review"; a locked title is never "Rename
 * pending"; Needs review counts uncertain matches, searches that found nothing and rename
 * conflicts. A New title without a flag is one no scrape was tried for yet: just New, "Not scraped
 * yet", never "Not matched" (QA final-07). (Every fixture title flagged Not matched waits in Needs
 * review, so `not_matched` and `could_not_match` agree here; in a real library a title whose review
 * was set aside stays Not matched without waiting in Needs review.)
 */

/** The facts a fixture title is generated from. */
export interface SyntheticLifecycleInput {
  /** 0-based position of the title in its fixture library. */
  index: number;
  title: string;
  year: number;
  /** The search found nothing for it. */
  unmatched: boolean;
  /** An uncertain match waits for a decision. */
  needsReview: boolean;
  show?: boolean;
}

const SCAN_MS = Date.UTC(2024, 9, 2, 10, 4);
const SCRAPE_MS = SCAN_MS + 4 * 60_000;

/** A small deterministic integer hash, so the same index always gives the same lifecycle. */
function mix(n: number): number {
  let h = Math.imul(n ^ 0x9e3779b9, 0x85ebca6b);
  h ^= h >>> 13;
  h = Math.imul(h, 0xc2b2ae35);
  return (h ^ (h >>> 16)) >>> 0;
}

type Organize = "tidy" | "pending" | "conflict" | "locked";

interface Shape {
  stage: StageDto;
  organize: Organize | null;
  flags: LifecycleFlagDto[];
}

function flag(kind: FlagKindDto, reason = ""): LifecycleFlagDto {
  return { kind, reason };
}

function shapeOf(input: SyntheticLifecycleInput): Shape {
  const h = mix(input.index + 31);
  if (input.unmatched) {
    return { stage: "new", organize: null, flags: [flag("not-matched", "No candidates found")] };
  }
  if (input.needsReview) {
    return { stage: "new", organize: null, flags: [flag("needs-review", "2 strong candidates")] };
  }
  if (h % 19 === 0) return { stage: "new", organize: null, flags: [] };
  if (h % 41 === 0) {
    return { stage: "scraped", organize: "locked", flags: [flag("locked")] };
  }
  if (h % 37 === 0) {
    return {
      stage: "scraped",
      organize: "pending",
      flags: [flag("missing", "The files are missing.")],
    };
  }
  if (h % 5 < 2) {
    return { stage: "scraped", organize: "pending", flags: [flag("rename-pending", "1 change")] };
  }
  return { stage: "renamed", organize: "tidy", flags: [] };
}

function scanLine(input: SyntheticLifecycleInput): LifecycleLineDto {
  const library = input.show === true ? "TV shows" : "Movies";
  const name = input.year > 0 ? `${input.title} (${input.year})` : input.title;
  return {
    step: "scan",
    status: "done",
    label: "Scanned",
    at_ms: SCAN_MS,
    parts: [{ text: `${library} › ${name}`, status: null }],
    link: null,
  };
}

function scrapeLine(input: SyntheticLifecycleInput, shape: Shape): LifecycleLineDto {
  if (shape.stage === "new") {
    const flagged = shape.flags[0];
    return {
      step: "scrape",
      status: flagged?.kind === "not-matched" ? "missing" : "waiting",
      label: flagged?.kind === "not-matched" ? "Not matched" : "Not scraped yet",
      at_ms: null,
      parts:
        flagged === undefined
          ? []
          : [
              {
                text:
                  flagged.kind === "not-matched"
                    ? flagged.reason
                    : `needs review: ${flagged.reason}`,
                status: null,
              },
            ],
      link: null,
    };
  }
  const h = mix(input.index + 7);
  return {
    step: "scrape",
    status: "done",
    label: "Scraped",
    at_ms: SCRAPE_MS,
    parts: [
      { text: `TMDB ${100 + (h % 900_000)}`, status: null },
      { text: "artwork saved", status: "done" },
      h % 3 === 0
        ? { text: "ratings 1 of 3", status: "needs-key" }
        : { text: "ratings 2 of 3", status: "waiting" },
      { text: "subtitles off", status: "off" },
      { text: "trailer off", status: "off" },
    ],
    link: null,
  };
}

function renameLine(shape: Shape): LifecycleLineDto {
  const base = { step: "rename", at_ms: null, link: null } as const;
  switch (shape.organize) {
    case "tidy":
      return {
        ...base,
        status: "done",
        label: "Renamed",
        parts: [{ text: "the names match the naming style", status: null }],
      };
    case "locked":
      return {
        ...base,
        status: "off",
        label: "Renamed: skipped",
        parts: [{ text: "locked", status: null }],
      };
    case "pending":
    case "conflict":
      return {
        ...base,
        status: "waiting",
        label: "Renamed: no",
        parts: [
          { text: "1 change pending in Organize", status: null },
          { text: "nothing moves until you apply it", status: null },
        ],
        link: "organize-preview",
      };
    case null:
      return { ...base, status: "off", label: "Renamed after scraping", parts: [] };
  }
}

function build(input: SyntheticLifecycleInput, shape: Shape): LifecycleDto {
  return {
    stage: shape.stage,
    flags: shape.flags,
    lines: [scanLine(input), scrapeLine(input, shape), renameLine(shape)],
    in_review: shape.flags.some(
      (entry) => entry.kind === "needs-review" || entry.kind === "not-matched",
    ),
  };
}

/** The lifecycle of one fixture title. */
export function syntheticLifecycle(input: SyntheticLifecycleInput): LifecycleDto {
  return build(input, shapeOf(input));
}

/** A scraped fixture title that is already renamed, or one whose rename waits in Organize. */
export function syntheticScrapedLifecycle(
  input: SyntheticLifecycleInput,
  renamed: boolean,
): LifecycleDto {
  return build(
    input,
    renamed
      ? { stage: "renamed", organize: "tidy", flags: [] }
      : { stage: "scraped", organize: "pending", flags: [flag("rename-pending", "1 change")] },
  );
}

/** The pipeline counts of the first `total` fixture titles, generated by `input`. */
export function syntheticLifecycleCounts(
  total: number,
  input: (index: number) => SyntheticLifecycleInput,
): LifecycleCountsDto {
  const counts: LifecycleCountsDto = {
    total,
    new: 0,
    scraped: 0,
    renamed: 0,
    needs_review: 0,
    uncertain: 0,
    not_matched: 0,
    not_scraped: 0,
    could_not_match: 0,
    not_matched_top_reason: null,
    working: 0,
    rename_pending: 0,
    locked: 0,
    missing: 0,
    checking: 0,
  };
  for (let index = 0; index < total; index += 1) {
    // The shape alone is enough to count; the explanation lines are built on demand.
    const shape = shapeOf(input(index));
    counts[shape.stage] += 1;
    const has = (kind: FlagKindDto) => shape.flags.some((entry) => entry.kind === kind);
    if (has("needs-review")) {
      counts.uncertain += 1;
      counts.needs_review += 1;
    }
    if (has("not-matched")) {
      counts.not_matched += 1;
      counts.could_not_match += 1;
      counts.needs_review += 1;
    }
    if (shape.stage === "new" && shape.flags.length === 0) counts.not_scraped += 1;
    if (has("rename-pending")) counts.rename_pending += 1;
    if (has("locked")) counts.locked += 1;
    if (has("missing")) counts.missing += 1;
  }
  if (counts.not_matched > 0) {
    counts.not_matched_top_reason = { reason: "No candidates found", count: counts.not_matched };
  }
  return counts;
}
