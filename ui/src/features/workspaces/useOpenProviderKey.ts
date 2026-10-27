import { useRouter } from "@tanstack/react-router";
import { useMemo } from "react";

/** Opens Settings, Providers at one provider's card, where the user adds or checks its key. */
export function useOpenProviderKey(): (provider: string) => void {
  const router = useRouter();
  return useMemo(
    () => (provider: string) => router.history.push(`/settings/providers#${provider}`),
    [router],
  );
}
