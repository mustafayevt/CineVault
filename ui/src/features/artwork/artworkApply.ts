import type { QueryClient } from "@tanstack/react-query";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { placementUndo, refreshArtwork, slotName } from "./artworkActions";
import { currentArtworkBackend } from "./artworkBackend";
import type { ArtworkCandidate, LocalImage } from "./artworkBackend";
import { patchMovieArtwork } from "./artworkOptimistic";

/** What the user picked to put in a type: a provider's image, a file of theirs, or a link. */
export type ArtworkChoice =
  | { kind: "candidate"; candidate: ArtworkCandidate }
  | { kind: "file"; image: LocalImage; name: string; url: string | null }
  | { kind: "link"; url: string };

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Puts a choice in a type of a title (spec `artwork` "Preview before an artwork is used"): a
 * provider's image shows on the title's pages at once from its cached thumbnail while the full
 * image is written, a toast confirms with Undo, and a refusal puts the old image back and says
 * why. The image it replaces goes to the library folder's trash folder. The placement is a
 * History entry, and the toast's Undo takes that entry back, as History does. Resolves to
 * whether it worked.
 */
export async function applyChoice(
  queryClient: QueryClient,
  itemId: string,
  title: string,
  slot: string,
  choice: ArtworkChoice,
): Promise<boolean> {
  const backend = currentArtworkBackend();
  const rollback =
    choice.kind === "candidate"
      ? patchMovieArtwork(queryClient, itemId, slot, choice.candidate)
      : () => undefined;
  const type = slotName(slot);
  try {
    const placed =
      choice.kind === "file"
        ? await backend.placeLocal(itemId, slot, choice.image)
        : await backend.place(
            itemId,
            slot,
            choice.kind === "candidate" ? choice.candidate.id : choice.url,
          );
    if (placed.images === 0) {
      rollback();
      notify({
        severity: "error",
        title: t("artworkPicker.toast.couldNot", { type, title }),
        ...(placed.problems[0] === undefined ? {} : { detail: placed.problems[0] }),
      });
      return false;
    }
    const undo = placementUndo(backend, queryClient, itemId, slot, placed);
    showToast({
      message: t("artworkPicker.toast.changed", { type, title }),
      ...(undo === undefined ? {} : { undo }),
    });
    for (const problem of placed.problems) {
      notify({ severity: "warning", title: t("artwork.toast.problem", { slot: type, problem }) });
    }
    await refreshArtwork(queryClient, itemId);
    return true;
  } catch (error) {
    rollback();
    notify({
      severity: "error",
      title: t("artworkPicker.toast.couldNot", { type, title }),
      detail: messageOf(error),
    });
    return false;
  }
}
