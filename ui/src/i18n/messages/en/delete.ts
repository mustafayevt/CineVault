/** English strings of the delete area. Merged with the other areas in `../../catalog.ts`. */
export const deleteMessages = {
  // Delete to the bin (spec title-deletion); `{bin}` is the Recycle Bin or the Trash, by platform
  "delete.toast.one": "Deleted {title} to the {bin}",
  "delete.toast.many": "Deleted {count} titles to the {bin}",
  "delete.toast.folder.one": "Deleted {title} to the library folder's trash folder",
  "delete.toast.folder.many": "Deleted {count} titles to the library folder's trash folder",
  "delete.toast.erased.one": "Erased {title}: Deleted files are set to Delete permanently",
  "delete.toast.erased.many": "Erased {count} titles: Deleted files are set to Delete permanently",
  "delete.toast.nothingOnDisk": "Removed {title}: it had no files on disk",
  "delete.refused":
    "{count, plural, one {# title was not deleted} other {# titles were not deleted}}",
  "delete.failed": "{subject} could not be deleted",

  // The Delete confirmation (bin and trash-folder modes)
  "delete.confirm.title.one": "Delete “{title}”?",
  "delete.confirm.title.many": "Delete {count} titles?",
  "delete.confirm.body.bin":
    "{count, plural, one {The folder goes} other {The folders go}} to the {bin}. You can undo this from History.",
  "delete.confirm.body.folder":
    "{count, plural, one {The folder goes to the trash folder on its drive} other {The folders go to the trash folder on their drives}}. You can undo this from History.",
  "delete.confirm.body.custom":
    "{count, plural, one {The folder goes} other {The folders go}} to your trash folder {folder} (files on other drives go to their own drive's trash folder). You can undo this from History.",
  "delete.confirm.body.customUnset":
    "{count, plural, one {The folder goes} other {The folders go}} to the trash folder on {count, plural, one {its drive} other {their drives}}, because no custom trash folder is chosen. You can undo this from History.",
  "delete.confirm.list": "What will be deleted",
  "delete.confirm.more": "{count, plural, one {and # more title} other {and # more titles}}",
  "delete.confirm.total": "Total",
  "delete.confirm.dontAsk": "Don't ask again",
  "delete.confirm.cancel": "Cancel",
  "delete.confirm.confirm": "Delete",

  // The permanent delete dialog
  "delete.dialog.title":
    "{count, plural, one {Delete # title permanently?} other {Delete # titles permanently?}}",
  "delete.dialog.body":
    "{count, plural, one {Its folder and all} other {Their folders and all}} {files} erased from disk. This skips the {bin} and cannot be undone.",
  "delete.dialog.files": "{count, plural, one {# file is} other {# files are}}",
  "delete.dialog.reading": "Reading what these titles hold",
  "delete.dialog.planFailed":
    "CineVault could not read what these titles hold, so nothing can be erased.",
  "delete.dialog.list": "What will be erased",
  "delete.dialog.shared": "Only its own files in this shared folder",
  "delete.dialog.root": "Only its own files; the library folder stays",
  "delete.dialog.noFolder": "Nothing on disk",
  "delete.dialog.skipped": "Not deleted",
  "delete.dialog.typeBefore": "Type",
  "delete.dialog.typeAfter": "to confirm",
  "delete.dialog.field": "Type DELETE to confirm",
  "delete.dialog.toBin": "Send to {bin} instead",
  "delete.dialog.cancel": "Cancel",
  "delete.dialog.confirm": "Delete permanently",

  // After a permanent delete
  "delete.erased":
    "{count, plural, one {Permanently deleted # title} other {Permanently deleted # titles}}",
  "delete.stopped": "{title} was left in the library",
  "delete.erase.refused": "{title} was not erased",
  "delete.erase.failed": "Nothing was erased",
} as const;
