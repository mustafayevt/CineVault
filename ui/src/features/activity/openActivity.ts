import { t } from "../../i18n/catalog";

/**
 * Opens Activity, at the row of an unfinished batch when one is given (Needs attention shows it
 * first and puts the focus on its first button). The router is loaded lazily: the stores that
 * call this are imported by the router's pages.
 */
export function openActivity(batchId: string | null = null): void {
  void import("../../router").then(({ router }) =>
    router.navigate({ to: "/activity", search: batchId === null ? {} : { batch: batchId } }),
  );
}

/** The "Open Activity" button of a toast about something that needs the user. */
export function openActivityAction(batchId: string | null = null): {
  label: string;
  onClick: () => void;
} {
  return { label: t("activity.toast.open"), onClick: () => openActivity(batchId) };
}

/** Opens Settings, Providers at one provider's key row, where a rejected key is put right. */
export function openProviderKey(provider: string): void {
  void import("../../router").then(({ router }) =>
    router.navigate({ to: "/settings", search: { section: "providers", key: provider } }),
  );
}
