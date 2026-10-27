import { useNavigate, useRouter } from "@tanstack/react-router";
import { useCallback } from "react";

/**
 * Closes a picker page (spec `ui-workspaces` "Pickers are pages"): back to the page it was opened
 * from, where its scroll position comes back with it; to the Library when the picker was the first
 * page of the session.
 */
export function usePickerClose(): () => void {
  const router = useRouter();
  const navigate = useNavigate();
  return useCallback(() => {
    if (router.history.canGoBack()) router.history.back();
    else void navigate({ to: "/library" });
  }, [router, navigate]);
}
