import { useQuery } from "@tanstack/react-query";

import { currentSharesBackend } from "./sharesBackend";

/** Query key of what this computer can do with network shares. */
export const SHARE_SUPPORT_KEY = ["shares", "support"] as const;

/** Query key of the shares CineVault connected. */
export const NETWORK_SHARES_KEY = ["shares", "list"] as const;

/** What this computer can do with network shares; it does not change while the app runs. */
export function useShareSupport() {
  return useQuery({
    queryKey: SHARE_SUPPORT_KEY,
    queryFn: () => currentSharesBackend().support(),
    staleTime: Infinity,
  });
}

/** The shares CineVault connected, with why each one is offline. */
export function useNetworkShares() {
  return useQuery({
    queryKey: NETWORK_SHARES_KEY,
    queryFn: () => currentSharesBackend().list(),
  });
}
