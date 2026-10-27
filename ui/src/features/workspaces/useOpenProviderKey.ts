import { useRouter } from "@tanstack/react-router";
import { useMemo } from "react";

/**
 * Opens Settings, Providers at one provider's key row, with the cursor in its field, where the
 * user adds or checks its key.
 */
export function useOpenProviderKey(): (provider: string) => void {
  const router = useRouter();
  return useMemo(
    () => (provider: string) =>
      void router.navigate({ to: "/settings", search: { section: "providers", key: provider } }),
    [router],
  );
}
