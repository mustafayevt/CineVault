import { Network } from "lucide-react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { Popover } from "../../components/Popover";
import { t } from "../../i18n/catalog";
import { isServer } from "../../lib/transport";
import { CopyCommand } from "./CopyCommand";

/** The installer of the server, which connects network shares there. */
const INSTALLER = "curl -fsSL https://mustafayevt.github.io/CineVault/install.sh | sh";

interface NetworkShareButtonProps {
  /** The button's words ("Network share…"). */
  children: string;
  /** Opens the Network share page (the desktop app). */
  onOpen: () => void;
}

/**
 * "Network share…". In the desktop app it opens the Network share page. In a browser it opens a
 * popover instead, with no password field: a server cannot connect a share itself, so its
 * installer does, and the folder is added with Add folder… afterwards.
 */
export function NetworkShareButton({ children, onOpen }: NetworkShareButtonProps) {
  if (!isServer()) {
    return (
      <Button icon={Network} onClick={onOpen}>
        {children}
      </Button>
    );
  }
  return (
    <Popover
      aria-label={t("networkShare.browser.label")}
      align="start"
      className="flex w-[420px] flex-col gap-3 p-3.5"
      trigger={<Button icon={Network}>{children}</Button>}
    >
      <div className="flex items-center gap-2">
        <Icon icon={Network} className="text-text-2" />
        <span className="font-(weight:--fw-600)">{t("networkShare.browser.title")}</span>
      </div>
      <p className="m-0 text-text-2">{t("networkShare.browser.body")}</p>
      <CopyCommand lines={[INSTALLER]} />
      <p className="m-0 text-text-2">{t("networkShare.browser.then")}</p>
    </Popover>
  );
}
