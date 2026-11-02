import { t } from "../../i18n/catalog";
import { useFormat } from "../../i18n/useFormat";
import { cx } from "../../lib/cx";
import { compareRows, verdictSentence } from "./ownedCopy";
import type { LibraryCopy, Verdict } from "./ownedCopy";
import type { TorrentRelease } from "./torrentBackend";

interface CompareBlockProps {
  copy: LibraryCopy;
  release: TorrentRelease;
  verdict: Verdict;
  /** Shows the "Compare with yours" heading (the details panel); a popover names it itself. */
  heading?: boolean;
}

/**
 * "Compare with yours" (spec `torrent-search` "Same, better or worse than yours"): the library
 * copy and the release in two columns, row by row, the rows that differ marked, and the plain
 * verdict under them. The verdict reads the backend's comparison, as the release's tag does.
 */
export function CompareBlock({ copy, release, verdict, heading = false }: CompareBlockProps) {
  const format = useFormat();
  const rows = compareRows(copy, release, (bytes) => format.size(bytes));
  return (
    <div data-testid="compare-block" className="flex flex-col gap-2">
      {heading ? (
        <h3 className="m-0 text-(length:--fs-13) font-(weight:--fw-600) text-text">
          {t("torrentsPage.cmp.title")}
        </h3>
      ) : null}
      <table className="w-full border-collapse text-(length:--fs-12)">
        <thead>
          <tr className="text-text-muted">
            <th scope="col" className="w-[84px] pb-1 text-left font-(weight:--fw-500)">
              <span className="sr-only">{t("torrentsPage.cmp.title")}</span>
            </th>
            <th scope="col" className="pb-1 text-left font-(weight:--fw-500)">
              {t("torrentsPage.cmp.yours")}
            </th>
            <th scope="col" className="pb-1 text-left font-(weight:--fw-500)">
              {t("torrentsPage.cmp.release")}
            </th>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={row.key}
              data-differs={row.differs ? "true" : undefined}
              className="border-t border-solid border-line-section"
            >
              <th
                scope="row"
                className="py-1 pr-2 text-left font-(weight:--fw-500) text-text-muted"
              >
                {t(`torrentsPage.cmp.row.${row.key}`)}
              </th>
              <td className="py-1 pr-2 text-text-2">{row.mine}</td>
              <td
                className={cx(
                  "py-1",
                  row.differs ? "font-(weight:--fw-600) text-text" : "text-text-2",
                )}
              >
                {row.theirs}
                {row.differs ? (
                  <span className="sr-only"> ({t("torrentsPage.cmp.differs")})</span>
                ) : null}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
      <p
        data-testid="compare-verdict"
        className={cx(
          "m-0 text-(length:--fs-13) font-(weight:--fw-600)",
          verdict === "better" ? "text-ok" : verdict === "worse" ? "text-warn" : "text-text-2",
        )}
      >
        {verdictSentence(verdict, copy, release)}
      </p>
    </div>
  );
}
