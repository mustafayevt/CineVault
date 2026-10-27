import { useEffect, useState } from "react";

import { currentArtworkBackend } from "./artworkBackend";
import type { ArtworkBackend, ArtworkImage } from "./artworkBackend";

/** Originals asked for, per backend and candidate, so a picture is asked for once. */
const asked = new WeakMap<ArtworkBackend, Map<string, Promise<ArtworkImage>>>();

/**
 * The key of the full-size original of a candidate (see `ArtworkBackend.fullImage`). The answer
 * is remembered, also while it is on its way; a failure is forgotten, so a later look tries again.
 */
export function fullImageOf(backend: ArtworkBackend, candidateId: string): Promise<ArtworkImage> {
  let forBackend = asked.get(backend);
  if (forBackend === undefined) {
    forBackend = new Map();
    asked.set(backend, forBackend);
  }
  const known = forBackend.get(candidateId);
  if (known !== undefined) return known;
  const pending = backend.fullImage(candidateId);
  const remembered = forBackend;
  remembered.set(candidateId, pending);
  pending.catch(() => {
    if (remembered.get(candidateId) === pending) remembered.delete(candidateId);
  });
  return pending;
}

/** How long a neighbour waits before its original is fetched, so flicking through asks for none. */
const PREFETCH_DELAY_MS = 350;

/** Where the original of the candidate on show stands. */
export type FullImageState =
  { status: "loading" } | { status: "ready"; key: string } | { status: "failed" };

/**
 * Fetches the original of the candidate on show, and then, after a short rest, those of its
 * `neighbours`, so the next Left or Right finds the picture ready. A neighbour that fails is left
 * alone: it is tried again when it is opened.
 */
export function useFullImage(
  candidateId: string | null,
  neighbours: readonly string[],
): FullImageState {
  const [state, setState] = useState<{ id: string; result: FullImageState } | null>(null);
  const near = neighbours.join("\n");

  useEffect(() => {
    if (candidateId === null) return undefined;
    let live = true;
    const backend = currentArtworkBackend();
    let timer: ReturnType<typeof setTimeout> | undefined;
    const prefetch = () => {
      timer = setTimeout(() => {
        for (const id of near === "" ? [] : near.split("\n")) {
          fullImageOf(backend, id).catch(() => undefined);
        }
      }, PREFETCH_DELAY_MS);
    };
    fullImageOf(backend, candidateId).then(
      (image) => {
        if (!live) return;
        setState({ id: candidateId, result: { status: "ready", key: image.key } });
        prefetch();
      },
      () => {
        if (!live) return;
        setState({ id: candidateId, result: { status: "failed" } });
        prefetch();
      },
    );
    return () => {
      live = false;
      if (timer !== undefined) clearTimeout(timer);
    };
  }, [candidateId, near]);

  return state?.id === candidateId ? state.result : { status: "loading" };
}
