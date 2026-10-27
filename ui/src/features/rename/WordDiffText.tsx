import { cx } from "../../lib/cx";
import { diffWords } from "./wordDiff";

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
}: {
  before: string;
  after: string;
  side: "before" | "after";
  className?: string;
}) {
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
      {segments.map((segment, index) =>
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
      )}
    </span>
  );
}
