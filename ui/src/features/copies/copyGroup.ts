import { useQuery } from "@tanstack/react-query";

import { t } from "../../i18n/catalog";
import { currentCopiesBackend } from "./copiesBackend";
import type { Comparison, CopyRow } from "./copiesBackend";
import { headingOf } from "./compareModel";
import { comparisonKey, useCopiesOf } from "./useCopiesOf";

/**
 * The tab order (spec `movie-copies` "Order and mark of the copy tabs"): the best-quality copy,
 * then the larger, then the lower library id. It does not depend on which copy is shown, so the
 * tabs stay put when the user switches.
 */
export function tabOrder(copies: readonly CopyRow[]): CopyRow[] {
  return [...copies].sort(
    (a, b) =>
      Number(b.best) - Number(a.best) ||
      (b.size_bytes ?? 0) - (a.size_bytes ?? 0) ||
      Number(a.id) - Number(b.id) ||
      a.id.localeCompare(b.id),
  );
}

/** The whole description of a copy, for the tooltip: "2160p · Dolby Vision · BluRay · 58.3 GB". */
export function tabLabel(copy: CopyRow, size: (bytes: number) => string): string {
  return [headingOf(copy), copy.source, copy.size_bytes === null ? "" : size(copy.size_bytes)]
    .filter((part) => part !== "")
    .join(" · ");
}

/** HDR formats as the short labels of the copy names use them. */
const HDR_SHORT: Readonly<Record<string, string>> = { "dolby vision": "DV" };

function pictureOf(copy: CopyRow): string {
  const media = copy.media;
  if (media === null) return t("copies.value.unread");
  const hdr = media.hdr_formats[0];
  return [
    media.resolution === "" ? t("copies.value.unknown") : media.resolution,
    hdr === undefined ? "" : (HDR_SHORT[hdr.toLowerCase()] ?? hdr),
  ]
    .filter((part) => part !== "")
    .join(" ");
}

/**
 * The texts of the tabs, short enough for the title line of the Technical card: the resolution
 * and the first HDR format ("2160p DV"), then the source and then the size added only where two
 * tabs would otherwise read the same ("1080p · BluRay", "1080p · WEB-DL").
 */
export function tabTexts(copies: readonly CopyRow[], size: (bytes: number) => string): string[] {
  const steps: ((copy: CopyRow) => string)[] = [
    (copy) => copy.source,
    (copy) => (copy.size_bytes === null ? "" : size(copy.size_bytes)),
  ];
  let texts = copies.map(pictureOf);
  for (const step of steps) {
    const clash = (text: string) => texts.filter((other) => other === text).length > 1;
    if (!texts.some(clash)) break;
    texts = texts.map((text, index) => {
      const copy = copies[index];
      const extra = copy === undefined ? "" : step(copy);
      return clash(text) && extra !== "" ? `${text} · ${extra}` : text;
    });
  }
  return texts;
}

/**
 * The copies of the shown movie in tab order, with what the Compare copies page knows about them;
 * `null` while the movie has no copy. The comparison is only asked for once the cheap copies
 * lookup says there are copies.
 */
export function useCopyGroup(movieId: string): Comparison | null {
  const of = useCopiesOf(movieId).data;
  const hasCopies = of !== undefined && of !== null;
  const page = useQuery({
    queryKey: comparisonKey(movieId),
    queryFn: () => currentCopiesBackend().compare(movieId),
    enabled: hasCopies,
    staleTime: 30_000,
  }).data;
  if (!hasCopies || page === undefined || page === null) return null;
  return { ...page, copies: tabOrder(page.copies) };
}
