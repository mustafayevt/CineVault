import { useEffect, useState } from "react";

import { t } from "../../i18n/catalog";
import { queryClient } from "../../queryClient";
import { offerNewKeyRatings } from "../ratings/ratingSweep";
import { showToast } from "../toasts/toastStore";
import { providersChanged } from "./useKeyAttention";
import { currentProvidersBackend } from "./providersBackend";
import type { ProviderCredentials, TestResult } from "./providersBackend";

/** The providers whose key unlocks a rating source (`cv_providers::RatingSource`). */
const RATING_KEYS: ReadonlySet<string> = new Set(["omdb", "mdblist"]);

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The providers' credentials as the cards show them, with the changes the cards make. */
export interface ProviderKeys {
  providers: ProviderCredentials[];
  loading: boolean;
  loadError: string | null;
  /** The last change in words, for the screen-reader status. */
  status: string;
  /** A change is running. */
  busy: boolean;
  available: boolean;
  save: (provider: ProviderCredentials, drafts: Record<string, string>) => Promise<string | null>;
  remove: (provider: ProviderCredentials) => Promise<string | null>;
  test: (
    provider: ProviderCredentials,
    drafts: Record<string, string>,
  ) => Promise<TestResult | string>;
}

/** Reads the providers' credentials and offers the changes (save, remove with Undo, test). */
export function useProviderKeys(): ProviderKeys {
  const backend = currentProvidersBackend();
  const [providers, setProviders] = useState<ProviderCredentials[]>([]);
  const [loading, setLoading] = useState(true);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [status, setStatus] = useState("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    let live = true;
    backend
      .list()
      .then((list) => {
        if (live) setProviders(list);
      })
      .catch((error: unknown) => {
        if (live) setLoadError(t("providers.loadFailed", { message: messageOf(error) }));
      })
      .finally(() => {
        if (live) setLoading(false);
      });
    return () => {
      live = false;
    };
  }, [backend]);

  /** Runs one change; returns the problem text, or null when it worked. */
  const change = async (
    run: () => Promise<ProviderCredentials[]>,
    done: string,
  ): Promise<string | null> => {
    setBusy(true);
    try {
      setProviders(await run());
      setStatus(done);
      providersChanged();
      return null;
    } catch (error) {
      return messageOf(error);
    } finally {
      setBusy(false);
    }
  };

  const save = async (provider: ProviderCredentials, drafts: Record<string, string>) => {
    setBusy(true);
    try {
      let list = providers;
      for (const [field, value] of Object.entries(drafts)) {
        list = await backend.set(provider.id, field, value);
      }
      setProviders(list);
      setStatus(t("providers.saved", { name: provider.name }));
      providersChanged();
      // A rating source's new key: titles scraped before can get its ratings now.
      if (RATING_KEYS.has(provider.id)) {
        void offerNewKeyRatings(queryClient, provider.id, provider.name);
      }
      return null;
    } catch (error) {
      return messageOf(error);
    } finally {
      setBusy(false);
    }
  };

  /** Removes the saved credentials and offers Undo, since a saved key cannot be shown again. */
  const remove = async (provider: ProviderCredentials): Promise<string | null> => {
    const problem = await change(() => backend.remove(provider.id), "");
    if (problem !== null) return problem;
    showToast({
      message: t("providers.removed", { name: provider.name }),
      undo: async () => {
        setProviders(await backend.restore(provider.id));
        providersChanged();
        showToast({ message: t("providers.restored", { name: provider.name }) });
      },
    });
    return null;
  };

  const test = async (
    provider: ProviderCredentials,
    drafts: Record<string, string>,
  ): Promise<TestResult | string> => {
    try {
      // The card's own status region says the result, so the page's stays quiet.
      const outcome = await backend.test(provider.id, drafts);
      // Testing the saved key can mark it Rejected or clear an earlier rejection.
      if (Object.keys(drafts).length === 0) setProviders(await backend.list());
      return outcome;
    } catch (error) {
      return messageOf(error);
    }
  };

  return {
    providers,
    loading,
    loadError,
    status,
    busy,
    available: backend.available,
    save,
    remove,
    test,
  };
}
