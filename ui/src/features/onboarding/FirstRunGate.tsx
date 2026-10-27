import { useNavigate, useRouterState } from "@tanstack/react-router";

import { AppShell } from "../shell/AppShell";
import { OnboardingPage } from "./OnboardingPage";
import { useFirstRunPhase } from "./useFirstRunPhase";

/**
 * The root of the window: the first-run setup, or the app shell. The setup replaces the whole
 * window while no library exists, and `/welcome` shows it on its own at any time.
 */
export function FirstRunGate() {
  const { phase, finish } = useFirstRunPhase();
  const navigate = useNavigate();
  const welcome = useRouterState({ select: (state) => state.location.pathname === "/welcome" });
  if (phase === "loading") return <div className="h-full bg-surface-window" aria-busy="true" />;
  if (phase === "setup" || welcome) {
    return (
      <OnboardingPage
        onFinished={(target) => {
          finish();
          void navigate({ to: "/library", search: { type: target === "tv" ? "tv" : "movies" } });
        }}
      />
    );
  }
  return <AppShell />;
}
