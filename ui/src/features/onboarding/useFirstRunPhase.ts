import { useEffect, useState } from "react";

import { currentSourcesBackend } from "../library-sources/sourcesBackend";
import { useSettingsStore } from "../settings/settingsStore";
import { firstRunPreview } from "./onboardingBackend";
import { ONBOARDING_DONE_KEY } from "./OnboardingPage";

type Phase = "loading" | "setup" | "done";

/**
 * Decides between the first-run setup and the app. The setup shows only when there is no library
 * (no folder yet) and it was not finished or left before. Where there is no library to ask, as in a
 * plain browser, the app opens at once; `?first-run` previews the setup there.
 */
export function useFirstRunPhase(): { phase: Phase; finish: () => void } {
  const [phase, setPhase] = useState<Phase>(() =>
    currentSourcesBackend().available ? "loading" : "done",
  );

  useEffect(() => {
    if (phase !== "loading") return;
    let live = true;
    const decide = async () => {
      try {
        await useSettingsStore.getState().load();
        const sources = await currentSourcesBackend().list();
        const completed = useSettingsStore.getState().values[ONBOARDING_DONE_KEY] === true;
        const needed = sources.length === 0 && (!completed || firstRunPreview());
        if (live) setPhase(needed ? "setup" : "done");
      } catch {
        // A library that cannot be read must not lock the user out of the app.
        if (live) setPhase("done");
      }
    };
    void decide();
    return () => {
      live = false;
    };
  }, [phase]);

  return { phase, finish: () => setPhase("done") };
}
