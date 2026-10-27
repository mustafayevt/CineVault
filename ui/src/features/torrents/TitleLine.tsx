import { X } from "lucide-react";

import { IconButton } from "../../components/IconButton";
import { t } from "../../i18n/catalog";

interface TitleLineProps {
  /** "Dune: Part Two (2024)". */
  name: string;
  kind: "movie" | "show";
  /** Takes the title away: the results go and the field is ready for another. */
  onChange: () => void;
}

/**
 * "Releases for [Dune: Part Two (2024) · Movie ×]" above the results: the title the person picked
 * from the suggestions, as a chip with an × to change it (spec `torrent-search` "Matched title
 * line"). The line is left out while no title is picked.
 */
export function TitleLine({ name, kind, onChange }: TitleLineProps) {
  // No title yet: no line at all, so the field and the chips sit 10px apart.
  if (name === "") return null;
  return (
    <div className="flex h-7 items-center gap-2 text-(length:--fs-12) text-text-muted">
      <span>{t("torrentsPage.match.for")}</span>
      <span
        data-testid="picked-title"
        className="inline-flex h-6 max-w-full min-w-0 items-center gap-1.5 rounded-(--r-pill) border border-solid border-line-edge bg-surface-card pl-2.5 text-text"
      >
        <span className="truncate font-(weight:--fw-600)">{name}</span>
        <span className="shrink-0 text-text-muted">
          · {t(kind === "show" ? "torrents.title.show" : "torrents.title.movie")}
        </span>
        <IconButton
          aria-label={t("torrentsPage.match.change")}
          icon={X}
          size="sm"
          onClick={onChange}
        />
      </span>
    </div>
  );
}
