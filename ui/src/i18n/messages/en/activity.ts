/** English strings of the Activity page's Needs attention and of unfinished moves. */
export const activity = {
  // The page and its sidebar entry
  "activity.region": "Activity",
  "activity.nav": "Activity",
  "activity.nav.attention":
    "{count, plural, one {Activity, # thing needs attention} other {Activity, # things need attention}}",

  // Needs attention
  "activity.attention": "Needs attention",
  "activity.attention.count": "{count, plural, one {# item} other {# items}}",
  "activity.attention.dismissAll": "Dismiss all",
  "activity.attention.dismissAllHint": "Clears the warnings; unfinished moves stay until resolved",
  "activity.history": "History",

  // An unfinished batch
  "activity.batch.move": "Move to {destination} did not finish",
  "activity.batch.moveNoPlace": "A move to another folder did not finish",
  "activity.batch.rename": "A rename did not finish",
  "activity.batch.running": "Resuming the move to {destination}",
  "activity.batch.runningNoPlace": "Resuming the move",
  "activity.batch.titles": "{first} and {count, plural, one {# more} other {# more}}",
  "activity.batch.files": "{done} of {total} files copied",
  "activity.batch.filesBytes": "{done} of {total} files copied, {bytesDone} of {bytesTotal}",
  "activity.batch.renamed": "{done} of {total} titles renamed",
  "activity.batch.resume": "Resume",
  "activity.batch.rollBack": "Roll back",
  "activity.batch.pause": "Pause",
  "activity.batch.paused": "Paused",
  "activity.batch.continue": "Continue",
  "activity.batch.cancel": "Cancel",
  "activity.batch.cancelHint": "Stops the move and takes this title back to how it was",
  "activity.batch.resumeHint": "Checks the files already copied, then copies the rest",
  "activity.batch.rollBackHint": "Removes only what this move made; the originals stay",
  "activity.batch.busy": "Another move is running. Wait for it to finish.",

  // A notice
  "activity.notice.dismiss": "Dismiss",
  "activity.notice.dismissLabel": "Dismiss “{title}”",
  "activity.notice.openKey": "Open the key",
  "activity.notice.retry": "Try again",
  "activity.notice.repeats": "×{count}",

  // What Resume and Roll back say
  "activity.toast.resuming": "Resuming the move of {label}",
  "activity.toast.resumed": "Finished the move of {label}",
  "activity.toast.resumedRename": "Finished the rename",
  "activity.toast.stoppedAgain": "The move of {label} stopped again: {reason}",
  "activity.toast.rolledBack": "Rolled back the move of {label}",
  "activity.toast.rolledBackRename": "Rolled back the rename",
  "activity.toast.open": "Open Activity",
  "activity.toast.unfinished": "{title}. Resume it or roll it back in Activity.",
  "activity.notice.resumeFailed": "The move could not be resumed",
  "activity.notice.rollBackFailed": "The move could not be rolled back",
  "activity.notice.rollBackProblems":
    "{count, plural, one {# thing could not be taken back} other {# things could not be taken back}}",
  "activity.notice.resumeNotes": "The resumed move left notes",

  // The Move to menu
  "activity.move.blocked": "A move of {label} to {destination} did not finish ({progress}).",
  "activity.move.blockedNoPlace": "A move of {label} did not finish ({progress}).",
  "activity.move.resume": "Resume that move",
  "activity.move.rollBack": "Roll it back",
  "activity.move.open": "Open in Activity",
} as const;
