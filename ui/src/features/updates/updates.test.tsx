import { act, render, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useNotificationStore } from "../notifications/notificationStore";
import { settingDef } from "../settings/schema";
import { useSettingsStore } from "../settings/settingsStore";
import { useToastStore } from "../toasts/toastStore";
import { JOBS_POLL_MS, useUpdateStore } from "./updateStore";
import { UpdateRequestError, setUpdatesBackend } from "./updatesBackend";
import type {
  UpdateCheck,
  UpdateInstall,
  UpdateOffer,
  UpdateProgress,
  UpdateStatus,
  UpdatesBackend,
} from "./updatesBackend";
import { FIRST_CHECK_DELAY_MS, useUpdateChecks } from "./useUpdateChecks";

const OFFER: UpdateOffer = {
  version: "1.2.0",
  current: "1.0.0",
  published: "2026-11-01T10:00:00Z",
  notes: "Faster scans.\nA fix for renamed folders.",
  size_bytes: 12_000_000,
};

interface Rig extends UpdatesBackend {
  checks: boolean[];
  /** Each install as `<version>`, or `<version> stop` when the jobs were to be stopped. */
  installs: string[];
}

const RESTARTING: UpdateInstall = { restarting: true, reason: null };

function rig(options: {
  channelSet?: boolean;
  enabled?: boolean;
  check?: () => Promise<UpdateCheck>;
  install?: (
    version: string,
    stopJobs: boolean,
    onProgress: (event: UpdateProgress) => void,
  ) => Promise<UpdateInstall>;
  blockingJobs?: () => Promise<number>;
}): Rig {
  const { channelSet = true, enabled = true } = options;
  const checks: boolean[] = [];
  const installs: string[] = [];
  const status: UpdateStatus = {
    current_version: "1.0.0",
    channel_set: channelSet,
    enabled,
    offer: null,
  };
  return {
    available: true,
    checks,
    installs,
    status: () => Promise.resolve(status),
    check: (manual) => {
      checks.push(manual);
      return options.check?.() ?? Promise.resolve({ outcome: "available", offer: OFFER });
    },
    install: (version, stopJobs, onProgress) => {
      installs.push(stopJobs ? `${version} stop` : version);
      return options.install?.(version, stopJobs, onProgress) ?? Promise.resolve(RESTARTING);
    },
    cancel: () => Promise.resolve(),
    blockingJobs: options.blockingJobs ?? (() => Promise.resolve(0)),
  };
}

beforeEach(() => {
  useUpdateStore.setState({
    phase: "idle",
    offer: null,
    progress: null,
    error: null,
    jobsMessage: null,
    reason: null,
    checking: false,
    checkResult: null,
    lastCheckedAt: null,
  });
  useToastStore.setState({ toasts: [] });
  useNotificationStore.setState({ items: [] });
});

afterEach(() => {
  setUpdatesBackend(null);
  vi.useRealTimers();
});

describe("update settings", () => {
  it("default to on, the built-in channel (an empty setting) and a daily interval", () => {
    expect(settingDef("general.check-updates")?.default).toBe(true);
    expect(settingDef("general.update-channel")?.default).toBe("");
    expect(settingDef("general.update-interval")?.default).toBe("daily");
    const control = settingDef("general.update-interval")?.control;
    expect(control?.type === "choice" && control.options.map((o) => o.value)).toEqual([
      "daily",
      "weekly",
      "monthly",
    ]);
  });
});

describe("background update check", () => {
  it("asks for nothing when no channel is set", async () => {
    const backend = rig({ channelSet: false });
    setUpdatesBackend(backend);
    await useUpdateStore.getState().checkInBackground();
    expect(backend.checks).toEqual([]);
    expect(useUpdateStore.getState().phase).toBe("idle");
  });

  it("asks for nothing when the toggle is off", async () => {
    const backend = rig({ enabled: false });
    setUpdatesBackend(backend);
    await useUpdateStore.getState().checkInBackground();
    expect(backend.checks).toEqual([]);
  });

  it("raises a toast and keeps the offer when a newer release is found, downloading nothing", async () => {
    const backend = rig({});
    setUpdatesBackend(backend);
    await useUpdateStore.getState().checkInBackground();
    expect(backend.checks).toEqual([false]);
    const state = useUpdateStore.getState();
    expect(state.phase).toBe("available");
    expect(state.offer?.version).toBe("1.2.0");
    expect(state.lastCheckedAt).not.toBeNull();
    expect(useToastStore.getState().toasts.map((toast) => toast.message)).toEqual([
      "CineVault 1.2.0 is available",
    ]);
    expect(backend.installs).toEqual([]);
  });

  it("stays silent when the check fails", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => undefined);
    setUpdatesBackend(
      rig({ check: () => Promise.reject(new UpdateRequestError("network", "offline")) }),
    );
    await useUpdateStore.getState().checkInBackground();
    expect(useUpdateStore.getState().phase).toBe("idle");
    expect(useNotificationStore.getState().items).toEqual([]);
    expect(useToastStore.getState().toasts).toEqual([]);
    warn.mockRestore();
  });

  it("starts once the settings are loaded, after a short delay", async () => {
    vi.useFakeTimers();
    const backend = rig({});
    setUpdatesBackend(backend);
    useSettingsStore.setState({ values: {}, status: "ready" });
    function Host() {
      useUpdateChecks();
      return null;
    }
    render(<Host />);
    expect(backend.checks).toEqual([]);
    await act(async () => {
      await vi.advanceTimersByTimeAsync(FIRST_CHECK_DELAY_MS + 10);
    });
    expect(backend.checks).toEqual([false]);
    useSettingsStore.setState({ status: "idle" });
  });
});

describe("installing an update", () => {
  async function offered(backend: Rig) {
    setUpdatesBackend(backend);
    await useUpdateStore.getState().checkNow();
  }

  it("installs only after install() is called, and restarts", async () => {
    const backend = rig({});
    await offered(backend);
    expect(backend.installs).toEqual([]);
    expect(useUpdateStore.getState().phase).toBe("reviewing");
    await useUpdateStore.getState().install();
    expect(useUpdateStore.getState().phase).toBe("installing");
    expect(backend.installs).toEqual(["1.2.0"]);
  });

  it("says why the installer was opened when the copy cannot install itself", async () => {
    await offered(
      rig({
        install: () =>
          Promise.resolve({ restarting: false, reason: "This copy needs the installer." }),
      }),
    );
    await useUpdateStore.getState().install();
    expect(useUpdateStore.getState().phase).toBe("started");
    expect(useUpdateStore.getState().reason).toBe("This copy needs the installer.");
  });

  it("asks before stopping a running job, and can update now and stop it", async () => {
    const backend = rig({
      install: (_version, stopJobs) =>
        stopJobs
          ? Promise.resolve(RESTARTING)
          : Promise.reject(new UpdateRequestError("jobs_running", "A job is running.")),
    });
    await offered(backend);
    await useUpdateStore.getState().install();
    expect(useUpdateStore.getState().phase).toBe("jobs");
    expect(useUpdateStore.getState().jobsMessage).toBe("A job is running.");
    await useUpdateStore.getState().install(true);
    expect(useUpdateStore.getState().phase).toBe("installing");
    expect(backend.installs).toEqual(["1.2.0", "1.2.0 stop"]);
  });

  it("updates by itself once the running jobs finish", async () => {
    const busy = [2, 1, 0];
    let tries = 0;
    const backend = rig({
      install: () => {
        tries += 1;
        return tries === 1
          ? Promise.reject(new UpdateRequestError("jobs_running", "2 jobs are running."))
          : Promise.resolve(RESTARTING);
      },
      blockingJobs: () => Promise.resolve(busy.shift() ?? 0),
    });
    await offered(backend);
    await useUpdateStore.getState().install();
    vi.useFakeTimers({ shouldAdvanceTime: true });
    act(() => {
      useUpdateStore.getState().waitForJobs();
    });
    expect(useUpdateStore.getState().phase).toBe("waiting");
    await act(async () => {
      await vi.advanceTimersByTimeAsync(JOBS_POLL_MS * 3 + 10);
    });
    await waitFor(() => {
      expect(useUpdateStore.getState().phase).toBe("installing");
    });
    // Only the first try and the one after the jobs finished; nothing while waiting.
    expect(backend.installs).toEqual(["1.2.0", "1.2.0"]);
  });

  it("can stop waiting for the jobs", async () => {
    const backend = rig({
      install: () => Promise.reject(new UpdateRequestError("jobs_running", "A job is running.")),
      blockingJobs: () => Promise.resolve(1),
    });
    await offered(backend);
    await useUpdateStore.getState().install();
    act(() => {
      useUpdateStore.getState().waitForJobs();
    });
    act(() => {
      useUpdateStore.getState().stopWaiting();
    });
    expect(useUpdateStore.getState().phase).toBe("jobs");
    expect(backend.installs).toEqual(["1.2.0"]);
  });

  it("explains a refused download and returns to the question", async () => {
    await offered(
      rig({
        install: () => Promise.reject(new UpdateRequestError("checksum", "Checksum mismatch.")),
      }),
    );
    await useUpdateStore.getState().install();
    expect(useUpdateStore.getState().phase).toBe("reviewing");
    expect(useUpdateStore.getState().error).toBe("Checksum mismatch.");
  });
});

describe("check now", () => {
  it("records when the check ended and says so when CineVault is up to date", async () => {
    setUpdatesBackend(
      rig({ check: () => Promise.resolve({ outcome: "up_to_date", offer: null }) }),
    );
    await useUpdateStore.getState().checkNow();
    const state = useUpdateStore.getState();
    expect(state.checkResult).toEqual({ tone: "success", message: "CineVault is up to date" });
    expect(state.lastCheckedAt).not.toBeNull();
    expect(state.phase).toBe("idle");
  });

  it("answers with the reason when the check could not run", async () => {
    setUpdatesBackend(
      rig({ check: () => Promise.reject(new UpdateRequestError("no_channel", "x")) }),
    );
    await useUpdateStore.getState().checkNow();
    expect(useUpdateStore.getState().checkResult).toEqual({
      tone: "danger",
      message: "Could not check for updates. No update channel is set for this build.",
    });
    expect(useNotificationStore.getState().items).toHaveLength(1);
  });

  it("opens the offer when an update is found", async () => {
    setUpdatesBackend(rig({}));
    await useUpdateStore.getState().checkNow();
    expect(useUpdateStore.getState().phase).toBe("reviewing");
    expect(useUpdateStore.getState().offer?.version).toBe("1.2.0");
  });
});
