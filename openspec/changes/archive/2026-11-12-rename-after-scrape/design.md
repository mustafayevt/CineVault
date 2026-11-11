## Context

The after-scrape steps (`cv-app::actions::after_scrape`) already run in the background for every title a scrape was written into, and the batch engine's rename step (`automation::engine`) already renames a selection safely: it previews first, applies only `Change` rows, waits for scrape follow-ups (NFO, artwork) to finish so no write lands in the old folder, refuses while Metadata only is on, journals the batch and returns its id for Undo.

## Decisions

- **Reuse the rename step, not the Rename page.** The runner submits a one-step plan (`Step::Rename`, scope All, selection = the ids) to the app's `CommandBackend`. That gives the Activity job, the journal batch, the conflict rules and the follow-up wait for free, and there is no second rename path to keep correct.
- **Gather, then rename.** A scrape of fifty titles ends title by title. Each title joins a queue when its steps and its artwork download are over; the queue is handed over once nobody joined for 3 seconds. Fifty titles become one batch and one Undo, not fifty toasts.
- **Who runs it.** `CommandBackend::start` installs itself as the runner (weakly held), so the desktop app and the browser server both get it from the place that already owns the batch runner. Without a runner (the command line), the queue drops its titles: a command-line scrape renames nothing, as before.
- **Feedback through notices.** The runner publishes an information notice with `batch_id` and the new action `undo_rename`; the window shows it as a toast with Undo through the same undo as the Rename page (History entry first, journal otherwise), and does not keep it in the notification centre, so it cannot be undone twice. Held-back titles are one warning notice for Needs attention, listing up to 8 titles with their reasons.
- **Off by default, Metadata only wins.** `rename_after_scrape(values)` is true only when the switch is on and Metadata only is off; the engine checks Metadata only again when it runs.

## Risks

- A rename starts a few seconds after the scrape: a user who opens the Rename page at once may see the titles leave the plan while they look. The toast says what happened and Undo restores it.
- A title whose scrape picked the wrong match is renamed to the wrong name. Undo takes it back; the switch is off by default and its description says it renames by itself.
