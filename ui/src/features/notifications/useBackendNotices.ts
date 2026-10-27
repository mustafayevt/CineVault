import { Channel } from "@tauri-apps/api/core";
import { useEffect } from "react";

import { commands } from "../../bindings";
import type { NoticeDto } from "../../bindings";
import { notify, useNotificationStore } from "./notificationStore";

let opened = false;

/** Forgets that the stream was opened. Tests only. */
export function resetNoticeStream(): void {
  opened = false;
}

/**
 * Opens the backend's stream of notices once for the life of the window. Messages from
 * background work (a job that ended with failures, a provider that rejected a key, a damaged
 * settings file) land in the notification centre; nothing opens a dialog. The backend replays
 * what happened before the window listened, to the first subscriber only, so the stream is not
 * closed when the shell remounts.
 */
export function openNoticeStream(): void {
  if (opened) return;
  try {
    const channel = new Channel<NoticeDto>();
    channel.onmessage = (notice) => {
      if (notice.resolves) {
        // Not a message: what the notices of this topic said no longer holds.
        if (notice.topic !== null) useNotificationStore.getState().resolveTopic(notice.topic);
        return;
      }
      notify({
        severity: notice.severity,
        title: notice.title,
        ...(notice.detail === null ? {} : { detail: notice.detail }),
        ...(notice.source === null ? {} : { source: notice.source }),
        ...(notice.provider_key === null ? {} : { providerKey: notice.provider_key }),
        ...(notice.action === null ? {} : { action: notice.action }),
        ...(notice.batch_id === null ? {} : { batchId: notice.batch_id }),
        ...(notice.topic === null ? {} : { topic: notice.topic }),
      });
    };
    const request = commands.subscribeNotices(channel);
    opened = true;
    request.catch(() => {
      // Not running inside the desktop shell (for example a plain browser): no notices arrive.
      opened = false;
    });
  } catch {
    // A plain browser has no desktop shell, and a Channel cannot be created there.
  }
}

/** Shows messages from background work in the notification centre while the shell is mounted. */
export function useBackendNotices(): void {
  useEffect(() => {
    openNoticeStream();
  }, []);
}
