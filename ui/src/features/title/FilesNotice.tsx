import { TriangleAlert } from "lucide-react";

import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { t } from "../../i18n/catalog";
import type { FilesState } from "../detail/filesState";

/**
 * One calm line on a title page whose files cannot be reached: what is wrong, the last known
 * folder, and nothing else. Play says the same reason; Library's Problems filter lists every such
 * title. A warning is never colour alone: it has an icon and words.
 */
export function FilesNotice({ state, folder }: { state: FilesState; folder: string }) {
  const missing = state === "missing";
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
      </div>
    </div>
  );
}
