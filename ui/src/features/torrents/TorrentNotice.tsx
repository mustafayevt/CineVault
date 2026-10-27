import { Card } from "../../components/Card";
import { Button } from "../../components/Button";
import { t } from "../../i18n/catalog";
import { useSettingsStore } from "../settings/settingsStore";

import { NOTICE_KEY } from "./useTorrentNotice";

/**
 * The responsibility notice, shown once above the search on the first visit. Until it is
 * accepted nothing is searched and no source is contacted.
 */
export function TorrentNotice() {
  return (
    <Card
      role="group"
      aria-label={t("torrents.notice.label")}
      data-testid="torrent-notice"
      className="flex flex-wrap items-center gap-x-6 gap-y-3"
    >
      <p className="m-0 min-w-56 flex-1">{t("torrents.notice.text")}</p>
      <Button
        variant="primary"
        onClick={() => {
          useSettingsStore.getState().set(NOTICE_KEY, true);
        }}
      >
        {t("torrents.notice.accept")}
      </Button>
    </Card>
  );
}
