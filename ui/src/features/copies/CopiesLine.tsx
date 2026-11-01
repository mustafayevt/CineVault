import { Link } from "@tanstack/react-router";
import { Copy } from "lucide-react";

import { Icon } from "../../components/Icon";
import { t } from "../../i18n/catalog";
import { useCopiesOf } from "./useCopiesOf";

/**
 * "You have 2 copies · Compare" on the Movie page, for a movie with copies, kept or not (spec
 * `movie-copies` "Copies line on the Movie page"). Nothing for a movie without a copy.
 */
export function CopiesLine({ movieId }: { movieId: string }) {
  const copies = useCopiesOf(movieId).data;
  if (copies === undefined || copies === null) return null;
  return (
    <div
      data-testid="copies-line"
      className="flex max-w-[640px] items-center gap-2 border-t border-solid border-line-row pt-3.5"
    >
      <Icon icon={Copy} size={16} className="text-text-muted" />
      <span className="text-text-2">{t("copies.movie.line", { count: copies.ids.length })}</span>
      <span aria-hidden="true" className="text-text-muted">
        ·
      </span>
      <Link to="/movies/$movieId/copies" params={{ movieId }} className="cv-link">
        {t("copies.movie.compare")}
      </Link>
    </div>
  );
}
