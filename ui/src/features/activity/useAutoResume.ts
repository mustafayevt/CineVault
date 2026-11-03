import { useEffect } from "react";

import { autoResumeUnfinished } from "./resolveUnfinished";

/**
 * Once the window is up, resumes the moves the app's close or a crash cut short (see
 * [`autoResumeUnfinished`]). Mounted once in the app shell; it runs once per window.
 */
export function useAutoResume(): void {
  useEffect(() => {
    void autoResumeUnfinished();
  }, []);
}
