import { Channel } from "@tauri-apps/api/core";
import { useEffect } from "react";

import { commands } from "../../bindings";
import type { JobProgressUpdate } from "../../bindings";
import { useJobProgressStore } from "./jobProgressStore";

/**
 * Opens the backend's job progress channel while the calling component is mounted. The backend
 * sends the state of every job first and then only changes, at most 10 times a second per job.
 */
export function useJobProgressStream(): void {
  const apply = useJobProgressStore((state) => state.apply);

  useEffect(() => {
    let channel: Channel<JobProgressUpdate>;
    try {
      channel = new Channel<JobProgressUpdate>();
    } catch {
      // A plain browser has no desktop shell, and a Channel cannot be created there.
      return;
    }
    channel.onmessage = apply;
    commands.subscribeJobProgress(channel).catch(() => {
      // Not running inside the desktop shell (for example a plain browser): no progress arrives.
    });
    return () => {
      // The backend forgets a channel on its next send once nothing listens to it.
      channel.onmessage = () => {};
    };
  }, [apply]);
}
