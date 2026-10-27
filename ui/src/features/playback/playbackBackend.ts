import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";
import type { PlayOutcomeDto } from "../../bindings";

/** What pressing Play did: the player that started and how many parts it was given. */
export type PlayOutcome = PlayOutcomeDto;

/** A refusal from the backend: `message` is ready to show, `code` says why (`file_missing`, ...). */
export class PlaybackError extends Error {
  readonly code: string;

  constructor(code: string, message: string) {
    super(message);
    this.name = "PlaybackError";
    this.code = code;
  }
}

/** Where the Play action goes: the desktop app starts the external player. */
export interface PlaybackBackend {
  /**
   * Plays a movie or an episode in the external player the settings name. Stacked parts are
   * queued in order and a disc plays as its folder.
   */
  play: (id: string) => Promise<PlayOutcome>;
}

const tauriPlaybackBackend: PlaybackBackend = {
  play: async (id) => {
    const result = await commands.playItem(id);
    if (result.status === "error") {
      throw new PlaybackError(result.error.code, result.error.message);
    }
    return result.data;
  },
};

/** A plain browser has no player to start; the call succeeds so the page can be developed. */
const browserPlaybackBackend: PlaybackBackend = {
  play: () => Promise.resolve({ player: "system", parts: 1, opened: 1 }),
};

let override: PlaybackBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setPlaybackBackend(backend: PlaybackBackend | null): void {
  override = backend;
}

export function currentPlaybackBackend(): PlaybackBackend {
  if (override !== null) return override;
  return isTauri() ? tauriPlaybackBackend : browserPlaybackBackend;
}
