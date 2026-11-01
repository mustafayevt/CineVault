import { cx } from "../../lib/cx";
import { diffWords } from "./wordDiff";
import type { DiffSegment } from "./wordDiff";

/** The segments cut in two at a character offset of their joined text. */
function splitAt(segments: readonly DiffSegment[], offset: number): [DiffSegment[], DiffSegment[]] {
  const head: DiffSegment[] = [];
  const tail: DiffSegment[] = [];
  let seen = 0;
  for (const segment of segments) {
    const end = seen + segment.text.length;
    if (end <= offset) head.push(segment);
    else if (seen >= offset) tail.push(segment);
    else {
      head.push({ ...segment, text: segment.text.slice(0, offset - seen) });
      tail.push({ ...segment, text: segment.text.slice(offset - seen) });
    }
    seen = end;
  }
  return [head, tail];
}

/** The runs of a name: the ones that go struck through, the ones that come highlighted. */
function Segments({
  segments,
  side,
}: {
  segments: readonly DiffSegment[];
  side: "before" | "after";
}) {
  return segments.map((segment, index) =>
    segment.changed ? (
      side === "before" ? (
        <del key={index} className="text-text-muted line-through decoration-text-disabled">
          {segment.text}
        </del>
      ) : (
        <ins
          key={index}
          className="rounded-[3px] bg-[color-mix(in_srgb,var(--ok)_14%,transparent)] text-ok no-underline"
        >
          {segment.text}
        </ins>
      )
    ) : (
      <span key={index}>{segment.text}</span>
    ),
  );
}

/**
 * A name before and after, one side at a time: the words that go are struck through on the old
 * side, the words that come are highlighted on the new side, and whole words and their brackets
 * go together (spec `renamer` "Relative word-level rename diffs").
 */
export function WordDiffText({
  before,
  after,
  side,
  className,
  mark,
}: {
  before: string;
  after: string;
  side: "before" | "after";
  className?: string;
  /** A part of `after` to show as a copy label (" - 2160p DV"), apart from the word diff. */
  mark?: string | undefined;
}) {
  const at = side === "after" && mark !== undefined && mark !== "" ? after.lastIndexOf(mark) : -1;
  if (at >= 0 && mark !== undefined) {
    // The label is not a rename of the old name: the diff runs on the name without it.
    const plain = after.slice(0, at) + after.slice(at + mark.length);
    const [head, tail] = splitAt(diffWords(before, plain).after, at);
    return (
      <span title={after} className={cx("cv-selectable block truncate text-text", className)}>
        <Segments segments={head} side="after" />
        <mark
          data-testid="copy-label"
          className="rounded-[3px] bg-gold-tint text-gold shadow-[inset_0_-1px_0_var(--gold-tint-edge)]"
        >
          {mark}
        </mark>
        <Segments segments={tail} side="after" />
      </span>
    );
  }
  const diff = diffWords(before, after);
  const segments = side === "before" ? diff.before : diff.after;
  return (
    <span
      title={side === "before" ? before : after}
      className={cx(
        "cv-selectable block truncate",
        side === "before" ? "text-text-muted" : "text-text",
        className,
      )}
    >
      <Segments segments={segments} side={side} />
    </span>
  );
}
