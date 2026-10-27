import type { QueryClient } from "@tanstack/react-query";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import type { ArtworkBackend, ArtworkPlaced, LocalImage } from "./artworkBackend";
import { slotLabelKey } from "./artworkModel";

/** The name of a slot for a sentence: `poster`, or the slot id of a type this build does not know. */
export function slotName(slot: string): string {
  const key = slotLabelKey(slot);
  return (key === null ? slot : t(key)).toLowerCase();
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Redraws everything that shows the artwork of an item. Every show page is read again: a season's
 * images are shown by its show's page and its season pages, which are cached under the show.
 */
export async function refreshArtwork(queryClient: QueryClient, itemId: string): Promise<void> {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["movie-detail", itemId] }),
    queryClient.invalidateQueries({ queryKey: ["tv-show"] }),
    queryClient.invalidateQueries({ queryKey: ["library"] }),
  ]);
}

/** The file name of a picture the user chose or dropped. */
export function localName(image: LocalImage): string {
  return image.kind === "path" ? (image.path.split(/[\\/]/).pop() ?? image.path) : image.file.name;
}

/**
 * The Undo of a placement: moves the new files to the trash folder and brings the displaced ones
 * back, then redraws the item. `undefined` when the placement wrote nothing to undo.
 */
export function placementUndo(
  backend: ArtworkBackend,
  queryClient: QueryClient,
  itemId: string,
  slot: string,
  placed: ArtworkPlaced,
): (() => Promise<void>) | undefined {
  const token = placed.undo;
  if (token === null) return undefined;
  return async () => {
    try {
      const undone = await backend.undoPlace(token);
      await refreshArtwork(queryClient, itemId);
      for (const problem of undone.problems) {
        notify({
          severity: "warning",
          title: t("artwork.toast.problem", { slot: slotName(slot), problem }),
        });
      }
    } catch (error) {
      notify({
        severity: "error",
        title: t("artwork.toast.couldNotUndo", { slot: slotName(slot) }),
        detail: messageOf(error),
      });
    }
  };
}
