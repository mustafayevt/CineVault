/**
 * A path split in two: the folders above the last one, ending in their separator, and the last
 * folder's (or file's) name. A drive root or a bare name is all name.
 */
export function splitPath(path: string): { head: string; leaf: string } {
  const trimmed = path.replace(/[\\/]+$/, "");
  const cut = Math.max(trimmed.lastIndexOf("\\"), trimmed.lastIndexOf("/"));
  if (cut < 0) return { head: "", leaf: path };
  return { head: trimmed.slice(0, cut + 1), leaf: trimmed.slice(cut + 1) };
}
