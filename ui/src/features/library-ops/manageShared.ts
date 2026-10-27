import { queryClient } from "../../queryClient";
import { SOURCE_HEALTH_KEY } from "../scan/useSourceHealth";
import { LibraryOpsError } from "./libraryOpsBackend";

/**
 * Reads the library again after titles moved, left or were deleted, or a folder was relabelled:
 * the library views, the detail pages (and the Inspector, which reads the same keys), Home and the
 * pipeline counts.
 */
export function refreshLibrary(
  client: Pick<typeof queryClient, "invalidateQueries"> = queryClient,
) {
  return Promise.all([
    client.invalidateQueries({ queryKey: ["library"] }),
    client.invalidateQueries({ queryKey: ["movie-detail"] }),
    client.invalidateQueries({ queryKey: ["tv-show"] }),
    client.invalidateQueries({ queryKey: ["tv-episode"] }),
    client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY }),
    client.invalidateQueries({ queryKey: ["home"] }),
    client.invalidateQueries({ queryKey: ["lifecycle"] }),
  ]);
}

export function messageOf(error: unknown): string {
  return error instanceof LibraryOpsError || error instanceof Error ? error.message : String(error);
}
