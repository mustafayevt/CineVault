import { Check, Copy } from "lucide-react";
import { useEffect, useState } from "react";
import type { ReactNode } from "react";

import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";

/** The words of a command the person fills in, shown in gold. */
const PLACEHOLDER = /\b(?:SERVER|SHARE|USER)\b/;

function highlighted(line: string): ReactNode[] {
  return line.split(new RegExp(`(${PLACEHOLDER.source})`)).map((part, index) =>
    PLACEHOLDER.test(part) ? (
      <span key={index} className="text-gold">
        {part}
      </span>
    ) : (
      part
    ),
  );
}

interface CopyCommandProps {
  /** One entry per line of the command. */
  lines: readonly string[];
}

/** A command in mono on the well colour with a Copy button that says so on the button for two seconds. */
export function CopyCommand({ lines }: CopyCommandProps) {
  const [copied, setCopied] = useState(false);
  useEffect(() => {
    if (!copied) return undefined;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);
  return (
    <div className="flex items-start gap-2.5 rounded-(--r-6) border border-solid border-line-edge bg-surface-well py-2 pr-2 pl-3">
      <code className="cv-selectable min-w-0 flex-1 pt-0.5 font-mono text-(length:--fs-12) leading-[1.55] break-all text-text">
        {lines.map((line, index) => (
          <span key={index} className="block">
            {highlighted(line)}
          </span>
        ))}
      </code>
      <Button
        size="sm"
        icon={copied ? Check : Copy}
        aria-label={t("networkShare.copyCommand")}
        onClick={() => {
          void navigator.clipboard
            ?.writeText(lines.join("\n"))
            .then(() => setCopied(true))
            .catch(() => setCopied(false));
        }}
      >
        {copied ? t("networkShare.copied") : t("networkShare.copy")}
      </Button>
    </div>
  );
}
