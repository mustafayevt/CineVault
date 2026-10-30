import { commands } from "../../bindings";
import { transport } from "../../lib/transport";
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

/** A browser plays the file itself through the server's media route. */
const browserPlaybackBackend: PlaybackBackend = {
  play: (id) => {
    window.open(`/api/media/${encodeURIComponent(id)}/0`, "_blank", "noopener");
    return Promise.resolve({ player: "browser", parts: 1, opened: 1 });
  },
};

let override: PlaybackBackend | null = null;

/** Replaces the backend; pass `null` to go back to the default. Used by tests. */
export function setPlaybackBackend(backend: PlaybackBackend | null): void {
  override = backend;
}

export function currentPlaybackBackend(): PlaybackBackend {
  if (override !== null) return override;
  return transport() === "desktop" ? tauriPlaybackBackend : browserPlaybackBackend;
}
