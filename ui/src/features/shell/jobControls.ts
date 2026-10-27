import { isTauri } from "@tauri-apps/api/core";

import { commands } from "../../bindings";

/** Pause, resume and cancel for one job: the job IPC in the app, nothing in a plain browser. */
export interface JobControls {
  setPaused: (jobId: number, paused: boolean) => Promise<boolean>;
  cancel: (jobId: number) => Promise<boolean>;
}

const tauriJobControls: JobControls = {
  setPaused: (jobId, paused) => commands.setJobPaused(jobId, paused),
  cancel: (jobId) => commands.cancelJob(jobId),
};

const noJobControls: JobControls = {
  setPaused: () => Promise.resolve(false),
  cancel: () => Promise.resolve(false),
};

let override: JobControls | null = null;

/** Replaces the controls (tests, the plain-browser demo); `null` goes back to the default. */
export function setJobControls(controls: JobControls | null): void {
  override = controls;
}

export function currentJobControls(): JobControls {
  return override ?? (isTauri() ? tauriJobControls : noJobControls);
}
