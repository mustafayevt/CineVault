import { create } from "zustand";

import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { showToast } from "../toasts/toastStore";
import { UpdateRequestError, currentUpdatesBackend } from "./updatesBackend";
import type { UpdateOffer, UpdateProgress } from "./updatesBackend";

/**
 * Where the update notice is:
 * - `idle`: nothing to show;
 * - `available`: a newer signed release was found (the notice is up);
 * - `reviewing`: the person opened the release notes and is deciding;
 * - `downloading`: they pressed Update now; the installer is coming down and being verified;
 * - `jobs`: a job is running, and the person is asked whether to wait for it or stop it;
 * - `waiting`: the update starts by itself once no job is running;
 * - `installing`: the setup runs by itself and the app quits in a moment, then restarts;
 * - `started`: the installer was handed to the operating system (`reason` says why).
 */
export type UpdatePhase =
  | "idle"
  | "available"
  | "reviewing"
  | "downloading"
  | "jobs"
  | "waiting"
  | "installing"
  | "started";

/** How often "Update after it finishes" asks whether the jobs are done. */
export const JOBS_POLL_MS = 2_000;

/** What the last "Check now" found, shown next to the button. */
export interface CheckResult {
  tone: "success" | "info" | "danger";
  message: string;
}

interface UpdateState {
  phase: UpdatePhase;
  offer: UpdateOffer | null;
  progress: UpdateProgress | null;
  /** Why the last Update now did not install, shown on the card until the next try. */
  error: string | null;
  /** The backend's words for the running jobs (phase `jobs`). */
  jobsMessage: string | null;
  /** Why the installer was opened instead of installing by itself (phase `started`). */
  reason: string | null;
  /** A manual check is running. */
  checking: boolean;
  /** The answer of the last manual check, until the next one starts. */
  checkResult: CheckResult | null;
  /** When the last check that reached the channel (or was due) ended, in ms; `null` before one. */
  lastCheckedAt: number | null;
  /** Reads the status once and checks if due; stays silent on any failure. */
  checkInBackground: () => Promise<void>;
  /**
   * The "Check now" button: always answers next to the button (`checkResult`), opens the notice
   * when an update is found, and keeps a failure in the notification centre too.
   */
  checkNow: () => Promise<void>;
  review: () => void;
  /** Hides the notice until the next check finds the update again. */
  later: () => void;
  /**
   * Update now: download, verify and install. `stopJobs` is the answer "Update now and stop it"
   * to a running job; without it a running job asks first.
   */
  install: (stopJobs?: boolean) => Promise<void>;
  /** "Update after it finishes": installs by itself once no job is queued or running. */
  waitForJobs: () => void;
  /** Stops waiting for the jobs and goes back to the question. */
  stopWaiting: () => void;
  cancelDownload: () => Promise<void>;
}

let jobsTimer: number | null = null;

function clearJobsTimer(): void {
  if (jobsTimer !== null) {
    window.clearTimeout(jobsTimer);
    jobsTimer = null;
  }
}

/** Opens Settings at Updates, where the offer can be installed. */
function openUpdates(): void {
  void import("../../router").then(({ router }) =>
    router.navigate({ to: "/settings", search: { section: "updates" } }),
  );
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * The reason for the line next to Check now, which sits in Settings under the fields it names: a
 * missing channel or key is pointed at by field, not sent "to Settings".
 */
function hereReason(error: unknown): string {
  if (error instanceof UpdateRequestError) {
    if (error.code === "no_channel") return t("updates.check.noChannelHere");
    if (error.code === "no_key") return t("updates.check.noKeyHere");
  }
  return messageOf(error);
}

export const useUpdateStore = create<UpdateState>((set, get) => ({
  phase: "idle",
  offer: null,
  progress: null,
  error: null,
  jobsMessage: null,
  reason: null,
  checking: false,
  checkResult: null,
  lastCheckedAt: null,
  checkInBackground: async () => {
    const backend = currentUpdatesBackend();
    if (!backend.available || get().phase !== "idle") return;
    try {
      const status = await backend.status();
      if (!status.channel_set || !status.enabled) return;
      const found = status.offer ?? (await backend.check(false)).offer;
      set({ lastCheckedAt: Date.now() });
      if (found !== null && get().phase === "idle") {
        set({ phase: "available", offer: found });
        showToast({
          message: t("updates.title", { version: found.version }),
          action: { label: t("updates.show"), onClick: openUpdates },
        });
      }
    } catch (error) {
      // A background check that fails is not worth interrupting anyone; it tries again later.
      console.warn("Update check failed", error);
    }
  },
  checkNow: async () => {
    const backend = currentUpdatesBackend();
    if (!backend.available || get().checking) return;
    set({ checking: true, checkResult: null });
    try {
      const result = await backend.check(true);
      set({ lastCheckedAt: Date.now() });
      if (result.offer !== null) {
        set({
          phase: "reviewing",
          offer: result.offer,
          progress: null,
          error: null,
          checkResult: {
            tone: "info",
            message: t("updates.title", { version: result.offer.version }),
          },
        });
      } else {
        set({ checkResult: { tone: "success", message: t("updates.upToDate") } });
      }
    } catch (error) {
      set({
        checkResult: {
          tone: "danger",
          message: t("updates.checkFailedWith", { reason: hereReason(error) }),
        },
      });
      notify({
        severity: "error",
        title: t("updates.checkFailed"),
        detail: messageOf(error),
        source: t("updates.source"),
      });
    } finally {
      set({ checking: false });
    }
  },
  review: () => {
    if (get().offer !== null) set({ phase: "reviewing" });
  },
  later: () => {
    clearJobsTimer();
    set({ phase: "idle", progress: null, error: null, jobsMessage: null });
  },
  install: async (stopJobs = false) => {
    const { offer, phase } = get();
    if (offer === null || phase === "downloading" || phase === "installing") return;
    clearJobsTimer();
    set({
      phase: "downloading",
      progress: { downloaded: 0, total: offer.size_bytes },
      error: null,
      jobsMessage: null,
    });
    try {
      const result = await currentUpdatesBackend().install(offer.version, stopJobs, (progress) =>
        set({ progress }),
      );
      if (result.restarting) {
        set({ phase: "installing", progress: null });
      } else {
        set({ phase: "started", progress: null, reason: result.reason });
      }
    } catch (error) {
      if (error instanceof UpdateRequestError && error.code === "jobs_running") {
        set({ phase: "jobs", progress: null, jobsMessage: error.message });
        return;
      }
      set({ phase: "reviewing", progress: null });
      const cancelled = error instanceof UpdateRequestError && error.code === "cancelled";
      if (!cancelled) {
        set({ error: messageOf(error) });
        notify({
          severity: "error",
          title: t("updates.installFailed"),
          detail: messageOf(error),
          source: t("updates.source"),
        });
      }
    }
  },
  waitForJobs: () => {
    if (get().offer === null) return;
    clearJobsTimer();
    set({ phase: "waiting" });
    const poll = async () => {
      jobsTimer = null;
      if (get().phase !== "waiting") return;
      let busy = 1;
      try {
        busy = await currentUpdatesBackend().blockingJobs();
      } catch (error) {
        console.warn("Could not read the running jobs", error);
      }
      if (get().phase !== "waiting") return;
      if (busy === 0) {
        await get().install(false);
      } else {
        jobsTimer = window.setTimeout(() => void poll(), JOBS_POLL_MS);
      }
    };
    jobsTimer = window.setTimeout(() => void poll(), JOBS_POLL_MS);
  },
  stopWaiting: () => {
    clearJobsTimer();
    if (get().phase === "waiting") set({ phase: "jobs" });
  },
  cancelDownload: async () => {
    await currentUpdatesBackend().cancel();
  },
}));
