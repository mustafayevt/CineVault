import { cx } from "../lib/cx";
import { splitPath } from "../lib/paths";

interface PathTextProps {
  path: string;
  className?: string;
  /** Show the full path as a tooltip. Off where the row has its own. */
  tooltip?: boolean;
}

/**
 * A path on one line that is shortened in the middle. When it does not fit, the folders above the
 * last one give way, ending in an ellipsis, and the last folder's name stays readable, so folders
 * that share a long parent (`...\media\Movies`, `...\media\TV`) can still be told apart. The full
 * path is the tooltip and is what assistive technology reads.
 */
export function PathText({ path, className, tooltip = true }: PathTextProps) {
  const { head, leaf } = splitPath(path);
  return (
    <span className={cx("flex min-w-0", className)} title={tooltip ? path : undefined}>
      <span aria-hidden="true" className="min-w-0 truncate">
        {head}
      </span>
      <span aria-hidden="true" className="max-w-full shrink-0 truncate">
        {leaf}
      </span>
      <span className="sr-only">{path}</span>
    </span>
  );
}
