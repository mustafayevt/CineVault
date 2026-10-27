/** The path's separator segments, without empty ones. */
function segments(path: string): string[] {
  return path.split(/[\\/]+/).filter((part) => part !== "");
}

/** The folder's short name: its last segment, or the whole path for a drive root. */
export function defaultSourceLabel(path: string): string {
  return segments(path).at(-1) ?? path;
}
