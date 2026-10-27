import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";

import type { ArtworkCandidate, ArtworkImage } from "./artworkBackend";
import { patchMovieArtwork } from "./artworkOptimistic";

const OLD: ArtworkImage = { key: "a".repeat(32), thumbhash: null, ambient: null };
const CHOSEN: ArtworkImage = { key: "b".repeat(32), thumbhash: null, ambient: null };

function candidate(): ArtworkCandidate {
  return { image: CHOSEN, width: 3840, height: 2160 } as unknown as ArtworkCandidate;
}

describe("a chosen image shown before it is written", () => {
  // The owner's blurred backdrop: the hero showed the candidate under the same URL the written
  // file has, so the small picture it got first stayed after the file was written.
  it("is marked as a preview in the hero, and the page read anew is not", () => {
    const client = new QueryClient();
    client.setQueryData(["movie-detail", "7"], { poster: OLD, backdrop: OLD });

    patchMovieArtwork(client, "7", "fanart", candidate());

    const page = client.getQueryData<{ backdrop: ArtworkImage & { preview?: boolean } }>([
      "movie-detail",
      "7",
    ]);
    expect(page?.backdrop.key).toBe(CHOSEN.key);
    expect(page?.backdrop.preview).toBe(true);
    expect(CHOSEN).not.toHaveProperty("preview");
  });
});
