import { t } from "../../i18n/catalog";
import { notify } from "../notifications/notificationStore";
import { localName } from "./artworkActions";
import type { ArtworkChoice } from "./artworkApply";
import type { LocalImage } from "./artworkBackend";
import { checkSource, isImageName } from "./artworkModel";

/**
 * The choice a picture from the user's disk makes: a file dropped on the page or picked in the
 * system dialog. A browser hands over the file itself, so it can be previewed; the desktop window
 * hands over only its path. Anything that is not a picture is refused with a note.
 */
export function choiceOfImage(image: LocalImage): ArtworkChoice | null {
  const name = localName(image);
  if (!isImageName(name)) {
    notify({ severity: "warning", title: t("artwork.toast.notImage", { name }) });
    return null;
  }
  const url =
    image.kind === "file" && typeof URL.createObjectURL === "function"
      ? URL.createObjectURL(image.file)
      : null;
  return { kind: "file", image, name, url };
}

/** The choice a pasted address makes: a web address is a link, a path or `file:` address a file. */
export function choiceOfAddress(text: string): ArtworkChoice | null {
  const check = checkSource(text);
  if (!check.ok) return null;
  if (/^https?:/i.test(check.source)) return { kind: "link", url: check.source };
  return choiceOfImage({ kind: "path", path: check.source.replace(/^file:\/*/i, "") });
}

/** Lets go of what a choice holds in memory (the preview of a dropped file). */
export function releaseChoice(choice: ArtworkChoice | null): void {
  if (choice?.kind === "file" && choice.url !== null && typeof URL.revokeObjectURL === "function") {
    URL.revokeObjectURL(choice.url);
  }
}
