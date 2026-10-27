import { Copy } from "lucide-react";

import { IconButton } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { copyText } from "./copy";

interface CopyButtonProps {
  /** What is copied. */
  value: string;
  /** The button's accessible name: "Copy full path", "Copy Heat's TMDB ID". */
  label: string;
  /** The toast text; defaults to "Copied {value}". */
  message?: string;
  failure?: string;
  /**
   * Quiet until the row is hovered or the button is focused. The button is always in the tab
   * order, so the keyboard reaches it; only its look is hidden.
   */
  quiet?: boolean;
  className?: string;
}

/** One-click copy of a technical value, confirmed with a toast. */
export function CopyButton({
  value,
  label,
  message,
  failure,
  quiet = false,
  className,
}: CopyButtonProps) {
  return (
    <IconButton
      icon={Copy}
      aria-label={label}
      title={label}
      className={cx(
        "shrink-0",
        quiet &&
          "opacity-0 group-focus-within/copy:opacity-100 group-hover/copy:opacity-100 focus-visible:opacity-100",
        className,
      )}
      onClick={() =>
        void copyText(
          value,
          message ?? t("value.copied", { value }),
          failure ?? t("path.copyFailed"),
        )
      }
    />
  );
}
