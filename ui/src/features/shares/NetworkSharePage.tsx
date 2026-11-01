import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useSearch } from "@tanstack/react-router";
import { Network } from "lucide-react";

import { Icon } from "../../components/Icon";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { addLibraryFolders, foldersAdded, reloadFolders } from "../library-sources/addFolders";
import { currentOnboardingBackend } from "../onboarding/onboardingBackend";
import { currentScanBackend } from "../scan/scanBackend";
import { SOURCE_HEALTH_KEY } from "../scan/useSourceHealth";
import { usePickerClose } from "../scrape/usePickerClose";
import { showToast } from "../toasts/toastStore";
import { NetworkShareForm } from "./NetworkShareForm";
import type { ShareDone, ShareMode } from "./NetworkShareForm";
import { NETWORK_SHARES_KEY, useNetworkShares, useShareSupport } from "./shareQueries";
import { shareSubtitle, shareTitle } from "./shareMode";
import type { NetworkShareSearch } from "./shareMode";
import type { NetworkShare } from "./sharesBackend";

/** The header of the page: the network tile, the title and the line under it. */
export function SharePageHeader({ title, sub }: { title: string; sub: string }) {
  return (
    <div className="mx-6 flex h-12 shrink-0 items-center gap-2.5 border-b border-solid border-line-section">
      <span className="flex size-7 shrink-0 items-center justify-center rounded-(--r-6) bg-surface-active text-text-2">
        <Icon icon={Network} />
      </span>
      <div className="min-w-0 flex-1">
        <h1 className="m-0 text-(length:--fs-16) leading-[1.3] font-(weight:--fw-700)">{title}</h1>
        <div className="text-(length:--fs-12) text-text-muted">{sub}</div>
      </div>
    </div>
  );
}

/**
 * The Network share page (`/network-share`, spec `network-shares`): a page, like the other
 * pickers, opened from "Network share…" in Settings › Library folders (`?kind=` names the library
 * the folder goes to) or from "Connect…" on an offline share's folder (`?share=` names the
 * share). Cancel, Esc and Back return to where it was opened.
 */
export function NetworkSharePage() {
  const { kind, share: shareId } = useSearch({ strict: false }) as NetworkShareSearch;
  const client = useQueryClient();
  const close = usePickerClose();
  const support = useShareSupport();
  const shares = useNetworkShares();
  const health = useQuery({
    queryKey: SOURCE_HEALTH_KEY,
    queryFn: () => currentScanBackend().sourceHealth(),
  });
  const known: NetworkShare | undefined =
    shareId === undefined ? undefined : shares.data?.find((entry) => entry.id === shareId);
  const waiting = support.data === undefined || (shareId !== undefined && known === undefined);
  const mode: ShareMode | null = waiting
    ? null
    : known === undefined
      ? { kind: "connect", library: kind ?? null }
      : { kind: "again", share: known };
  const folders = (health.data ?? []).filter((row) => row.share?.id === shareId).length;

  const done = async (result: ShareDone) => {
    if (result.kind === "added") {
      await client.invalidateQueries({ queryKey: NETWORK_SHARES_KEY });
      await foldersAdded(client, 1);
    } else {
      await Promise.all([
        client.invalidateQueries({ queryKey: NETWORK_SHARES_KEY }),
        reloadFolders(client),
      ]);
      showToast({ message: t("networkShare.connected", { address: result.share.address }) });
    }
    close();
  };

  // On a Linux desktop that cannot connect shares the way on is the folder picker.
  const addFolder = () => {
    void currentOnboardingBackend()
      .pickFolders()
      .then((paths) => (paths.length > 0 ? addLibraryFolders(client, paths, kind ?? null) : 0))
      .then((added) => {
        if (added > 0) close();
      })
      .catch(() => undefined);
  };

  return (
    <section aria-label={t("networkShare.region")} className="flex min-h-full flex-col">
      <SharePageHeader
        title={mode === null ? t("networkShare.title") : shareTitle(mode)}
        sub={mode === null ? "" : shareSubtitle(mode, folders)}
      />
      <div className="flex max-w-[760px] flex-col gap-4 px-6 py-6">
        {mode === null || support.data === undefined ? (
          <div aria-busy="true">
            <Skeleton shape="tile" aspect="4 / 1" />
          </div>
        ) : (
          <NetworkShareForm
            key={known?.id ?? "new"}
            mode={mode}
            support={support.data}
            onDone={(result) => {
              void done(result);
            }}
            onCancel={close}
            onAddFolder={addFolder}
          />
        )}
      </div>
    </section>
  );
}
