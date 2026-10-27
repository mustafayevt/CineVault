import { cx } from "../lib/cx";

/** The floating line itself. It sits just below its positioned parent. */
export function ReasonTip({
  id,
  line,
  className,
}: {
  id: string;
  line: string;
  className?: string;
}) {
  return (
    <span
      id={id}
      role="tooltip"
      className={cx(
        "type-small bg-surface-raised text-text pointer-events-none absolute top-full left-0 z-50 mt-1 w-max max-w-72 rounded-(--r-6) px-2.5 py-1.5 shadow-floating",
        className,
      )}
    >
      {line}
    </span>
  );
}
