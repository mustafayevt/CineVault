import { commands } from "../../bindings";
import type { WatchStateDto } from "../../bindings";

/**
 * The watched state each changed title had before a change: the flag, the play count and the
 * last-watched date. An Undo gives this back through [`restoreWatchedStates`], so a play count
 * above 1 comes back as it was and not as 1.
 */
export type WatchedChange = readonly WatchStateDto[];

/** A refusal from putting the watched states back. */
class WatchedRestoreError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "WatchedRestoreError";
    this.code = code;
  }
}

/** Puts back the states a watched change reported (desktop app). */
export async function restoreWatchedStates(previous: WatchedChange): Promise<void> {
  if (previous.length === 0) return;
  const result = await commands.restoreWatched([...previous]);
  if (result.status === "error") {
    throw new WatchedRestoreError(result.error.code, result.error.message);
  }
}
