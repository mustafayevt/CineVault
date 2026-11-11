import { useNavigate } from "@tanstack/react-router";
import { FolderSync, TriangleAlert } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { t } from "../../i18n/catalog";
import type { FilesState } from "../detail/filesState";
import { TITLE_ACTIONS } from "../library/titleActions";
import { runScanTitle } from "../scan/scanActions";
import { removeTitlesFromLibrary } from "./removeTitles";

/**
 * One calm line on a title page whose files cannot be reached: what is wrong, the last known
 * folder, and nothing else. Play says the same reason; Library's Problems filter lists every such
 * title. A warning is never colour alone: it has an icon and words.
 *
 * Both offer "Scan again", which reads the title's folder at once, so files that are back (or a
 * drive plugged in again) show without a scan of the whole library. When the files are missing
 * (not when the drive is offline, which fixes itself) it also says why the title is kept and
 * offers "Remove from library (keep files)", the same action as the ⋯ menu, which then opens the
 * Library like the menu does.
 */
export function FilesNotice({
  state,
  folder,
  id,
  title,
}: {
  state: FilesState;
  folder: string;
  /** The title's id and name, for the removal. */
  id: string;
  title: string;
}) {
  const missing = state === "missing";
  const navigate = useNavigate();
  const remove = () => {
    void removeTitlesFromLibrary([id], false, title).then((removed) => {
      if (removed) void navigate({ to: "/library" });
    });
  };
  return (
    <div
      role="status"
      data-testid="files-notice"
      className="flex items-start gap-2.5 rounded-(--r-10) border border-solid border-line-section bg-warn-tint px-3.5 py-2.5"
    >
      <Icon icon={TriangleAlert} className="mt-0.5 shrink-0 text-warn" />
      <div className="flex min-w-0 flex-col gap-0.5">
        <span className="font-(weight:--fw-600)">
          {missing ? t("detail.files.missing") : t("detail.files.offline")}
        </span>
        <span className="text-(length:--fs-12) text-text-2">
          {missing ? t("detail.files.missingBody") : t("detail.files.offlineBody")}
        </span>
        {folder === "" ? null : (
          <span className="type-code cv-selectable flex gap-1 text-text-muted">
            <span className="shrink-0">{t("detail.files.lastKnown")}</span>
            <PathText path={folder} />
          </span>
        )}
        <div className="mt-2 flex flex-wrap gap-2">
          <Button
            size="sm"
            icon={FolderSync}
            aria-label={t("scan.againFor", { title })}
            onClick={() => void runScanTitle(id, title)}
            data-testid="files-notice-scan"
          >
            {t("scan.again")}
          </Button>
          {missing ? (
            <Button
              size="sm"
              icon={TITLE_ACTIONS["remove"].icon}
              onClick={remove}
              data-testid="files-notice-remove"
            >
              {t("title.menu.remove")}
            </Button>
          ) : null}
        </div>
      </div>
    </div>
  );
}
