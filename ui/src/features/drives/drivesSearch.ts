/** The search of `/drives`: `drive` is the id of the chosen drive. */
export interface DrivesSearch {
  drive?: string;
}

/** Reads the search of `/drives`; anything damaged is dropped. */
export function validateDrivesSearch(search: Record<string, unknown>): DrivesSearch {
  const drive = search.drive;
  // The router reads `?drive=1` as a number; a drive id is always text.
  if (typeof drive === "number") return { drive: String(drive) };
  return typeof drive === "string" && drive !== "" ? { drive } : {};
}

/** The drive the page shows: the chosen one, or the first while none is chosen or it went away. */
export function chosenDrive<T extends { id: string }>(
  drives: readonly T[],
  chosen: string | undefined,
): T | undefined {
  return drives.find((entry) => entry.id === chosen) ?? drives[0];
}
