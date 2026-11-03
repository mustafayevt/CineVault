import { Link } from "@tanstack/react-router";
import { Library } from "lucide-react";

import type { ScrapeLinkDto } from "../../bindings";
import { Icon } from "../../components/Icon";
import type { WishHeld } from "./torrentBackend";
import type { TitleKind } from "./torrentsSearch";
import { heldLine } from "./wishlistModel";

interface WishHeldLineProps {
  held: WishHeld;
  kind: TitleKind;
  /** The library title, linked when known. */
  library: ScrapeLinkDto | null;
}

/**
 * "You already have this: 1080p · WEB-DL" (spec `torrent-wishlist`), on an entry and in the
 * editor, linking to the Movie or TV show page.
 */
export function WishHeldLine({ held, kind, library }: WishHeldLineProps) {
  const text = heldLine(held);
  const className = "truncate";
  return (
    <p
      data-testid="wish-held"
      className="m-0 flex min-w-0 items-center gap-1.5 text-(length:--fs-12) text-text-2"
    >
      <Icon icon={Library} size={14} />
      {library === null ? (
        <span className={className}>{text}</span>
      ) : kind === "show" ? (
        <Link
          to="/tv/$showId"
          params={{ showId: library.itemId }}
          className={`${className} cv-link`}
        >
          {text}
        </Link>
      ) : (
        <Link
          to="/movies/$movieId"
          params={{ movieId: library.itemId }}
          className={`${className} cv-link`}
        >
          {text}
        </Link>
      )}
    </p>
  );
}
