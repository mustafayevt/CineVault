import type { ConflictDto, PreviewRowDto } from "../../bindings";
import { t } from "../../i18n/catalog";

/**
 * A path below the data source root written with the separator of the root (`D:\Movies\` gives
 * `Heat (1995)\Heat.mkv`), so a row never mixes `/` and `\`.
 */
export function displayPath(root: string, relative: string): string {
  return root.includes("\\") ? relative.replaceAll("/", "\\") : relative;
}

/** The last part of a path, whatever its separators. */
export function leafOf(path: string): string {
  return (
    path
      .split(/[\\/]+/)
      .filter((part) => part !== "")
      .at(-1) ?? path
  );
}

/** A library folder without its trailing separator: `D:\Movies\` is `D:\Movies`. */
export function rootText(root: string): string {
  return root.length > 1 ? root.replace(/[\\/]+$/, "") : root;
}

/** The whole path of a folder below a library root. */
export function fullPath(root: string, relative: string): string {
  const separator = root.includes("\\") ? "\\" : "/";
  const base = root.endsWith("\\") || root.endsWith("/") ? root : `${root}${separator}`;
  return `${base}${displayPath(root, relative)}`;
}

/** What a row's conflict says, in the row's own words. */
export function conflictLine(row: PreviewRowDto, conflict: ConflictDto): string {
  const name = leafOf(conflict.path);
  const folder = rootText(row.root);
  switch (conflict.code) {
    case "foreign":
      return t("renamePage.conflict.exists", { name, folder });
    case "duplicate":
      return t("renamePage.conflict.duplicate", { name, folder });
    case "path_too_long":
      return t("renamePage.conflict.tooLong", { length: conflict.length, limit: conflict.limit });
    case "name_too_long":
      return t("renamePage.conflict.nameTooLong", {
        length: conflict.length,
        limit: conflict.limit,
      });
    case "case_only":
      return t("renamePage.conflict.caseOnly");
  }
}
