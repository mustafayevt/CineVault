import { useNavigate } from "@tanstack/react-router";

import { SegmentedControl } from "../../components/SegmentedControl";
import { t } from "../../i18n/catalog";
import { useWishlistNewCount } from "./wishlistQueries";

type View = "search" | "wishlist";

/**
 * "Search · Wishlist" at the top of both Torrents views (spec `torrent-wishlist` "Wishlist
 * view"); the Wishlist segment counts the new matches.
 */
export function TorrentsViewSwitch({ value }: { value: View }) {
  const navigate = useNavigate();
  const fresh = useWishlistNewCount();
  return (
    <SegmentedControl<View>
      aria-label={t("wishlist.switch")}
      className="shrink-0"
      options={[
        { value: "search", label: t("wishlist.switch.search") },
        {
          value: "wishlist",
          label: t("wishlist.switch.wishlist"),
          ...(fresh > 0 ? { count: fresh } : {}),
        },
      ]}
      value={value}
      onChange={(view) =>
        void navigate({ to: view === "search" ? "/torrents" : "/torrents/wishlist" })
      }
    />
  );
}
