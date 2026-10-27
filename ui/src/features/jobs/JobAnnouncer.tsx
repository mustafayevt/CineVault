import { useEffect, useState } from "react";

import type { JobProgress } from "../../bindings";
import { OVER, jobEndAnnouncement } from "./jobAnnouncement";
import { useJobProgressStore } from "./jobProgressStore";

/**
 * Tells screen readers, politely, when a background job finishes or fails. It listens to the
 * jobs' states, not to their progress, so a running scan never speaks on every tick; jobs that
 * were already over when the app opened stay quiet, and a cancel the user asked for is not
 * repeated back. Mounted once, in the shell.
 */
export function JobAnnouncer() {
  // The count makes a repeated sentence a new node, so it is read again.
  const [message, setMessage] = useState({ text: "", count: 0 });

  useEffect(() => {
    // The state each job had when last seen; `null` until the first list arrives, which is
    // what was there when the app opened: nothing in it has just ended.
    let seen: Map<number, JobProgress["state"]> | null = null;
    const check = (jobs: readonly JobProgress[]) => {
      const lines: string[] = [];
      const next = new Map<number, JobProgress["state"]>();
      for (const job of jobs) {
        next.set(job.id, job.state);
        if (seen === null || !OVER.has(job.state)) continue;
        // Over now, and seen running before (or new since the app opened and over already).
        const before = seen.get(job.id);
        if (before !== undefined && OVER.has(before)) continue;
        const line = jobEndAnnouncement(job);
        if (line !== null) lines.push(line);
      }
      if (seen !== null || jobs.length > 0) seen = next;
      if (lines.length > 0) {
        setMessage((current) => ({ text: lines.join(" "), count: current.count + 1 }));
      }
    };
    check(useJobProgressStore.getState().jobs);
    return useJobProgressStore.subscribe((state, previous) => {
      if (state.jobs !== previous.jobs) check(state.jobs);
    });
  }, []);

  return (
    <div role="status" aria-live="polite" className="sr-only" data-testid="job-announcer">
      {message.text === "" ? null : <span key={message.count}>{message.text}</span>}
    </div>
  );
}
