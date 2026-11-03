import { useQuery, useQueryClient } from "@tanstack/react-query";
import type { QueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect } from "react";

import { t } from "../../i18n/catalog";
import { showToast } from "../toasts/toastStore";
import { currentTorrentBackend } from "./torrentBackend";
import { useTorrentNotice } from "./useTorrentNotice";

/** The query key of the Wishlist; the new-match count lives under it. */
export const WISHLIST_KEY = ["torrents", "wishlist"] as const;
const COUNT_KEY = [...WISHLIST_KEY, "new"] as const;

/** How often the app asks for a due-only check while it stays open. */
const WATCH_EVERY_MS = 60 * 60_000;

/** Every entry of the Wishlist with its matches. */
export function useWishlist(enabled = true) {
  return useQuery({
    enabled,
    queryKey: WISHLIST_KEY,
    queryFn: () => currentTorrentBackend().wishlist(),
    retry: false,
  });
}

/** How many matches have not been looked at yet; 0 while unknown or when it cannot be read. */
export function useWishlistNewCount(): number {
  const accepted = useTorrentNotice();
  const query = useQuery({
    enabled: accepted === true,
    queryKey: COUNT_KEY,
    queryFn: () => currentTorrentBackend().wishlistNewCount(),
    retry: false,
  });
  return query.data ?? 0;
}

/** Reads the Wishlist and its count again. */
export function refreshWishlist(client: QueryClient): Promise<void> {
  return client.invalidateQueries({ queryKey: WISHLIST_KEY });
}

/** Says how many new matches a check found, with "Show" to open the Wishlist. */
export function announce(newMatches: number, open: () => void): void {
  if (newMatches <= 0) return;
  showToast({
    message: t("wishlist.toast.found", { count: newMatches }),
    duration: null,
    action: { label: t("wishlist.toast.show"), onClick: open },
  });
}

/**
 * Checks the entries that are due once the app opens and every hour while it stays open (each
 * entry at most every 6 hours; the backend decides), and announces new matches. Nothing runs
 * before the Torrent search notice is accepted.
 */
export function useWishlistWatch(): void {
  const accepted = useTorrentNotice();
  const client = useQueryClient();
  const navigate = useNavigate();
  useEffect(() => {
    if (accepted !== true) return;
    let stopped = false;
    const check = async () => {
      try {
        const report = await currentTorrentBackend().wishlistCheck(true);
        if (stopped || report.checked === 0) return;
        await refreshWishlist(client);
        announce(report.newMatches, () => void navigate({ to: "/torrents/wishlist" }));
      } catch {
        // A check that cannot run (no library, no key) is tried again at the next hour.
      }
    };
    void check();
    const timer = window.setInterval(() => void check(), WATCH_EVERY_MS);
    return () => {
      stopped = true;
      window.clearInterval(timer);
    };
  }, [accepted, client, navigate]);
}
