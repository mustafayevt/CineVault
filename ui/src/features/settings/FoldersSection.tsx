import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Plus } from "lucide-react";
import { useCallback, useState } from "react";

import { Button } from "../../components/Button";
import { PathText } from "../../components/PathText";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { currentSourcesBackend } from "../library-sources/sourcesBackend";
import type { LibrarySource, SourceKind } from "../library-sources/sourcesBackend";
import { refreshLibrary } from "../library-ops/manageShared";
import { currentOnboardingBackend } from "../onboarding/onboardingBackend";
import { useFolderDrop } from "../onboarding/useFolderDrop";
import { runRescan } from "../scan/scanActions";
import { currentScanBackend } from "../scan/scanBackend";
import type { SourceHealth } from "../scan/scanBackend";
import { SOURCE_HEALTH_KEY, SOURCE_LABELS_KEY } from "../scan/useSourceHealth";
import { showToast } from "../toasts/toastStore";
import { FormRow, SettingsSection } from "./FormParts";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The line under a folder: what it holds and whether it is watched, or that it is offline. */
function infoOf(source: LibrarySource, health: SourceHealth | undefined): string {
  if (health === undefined) return "";
  const count = health.titles;
  if (!health.online) {
    return t(
      source.kind === "tv"
        ? "settingsPage.folders.offline.shows"
        : "settingsPage.folders.offline.titles",
      { count },
    );
  }
  const held = t(
    source.kind === "tv" ? "settingsPage.folders.shows" : "settingsPage.folders.titles",
    { count },
  );
  return `${held} · ${t("settingsPage.folders.watched")}`;
}

interface FolderRowProps {
  source: LibrarySource;
  /** The kind shown: the one just chosen while the change is being saved. */
  kind: SourceKind;
  health: SourceHealth | undefined;
  onKind: (kind: SourceKind) => void;
  onRemove: () => void;
}

function FolderRow({ source, kind, health, onKind, onRemove }: FolderRowProps) {
  const offline = health !== undefined && !health.online;
  return (
    <FormRow
      dim={offline}
      label={
        <PathText
          path={source.path}
          className={cx("font-mono text-(length:--fs-12)", !offline && "text-text")}
        />
      }
      hint={<span className="block min-h-4">{infoOf(source, health)}</span>}
    >
      <div className="flex items-center gap-2" data-source={source.id}>
        <SegmentedControl<SourceKind>
          aria-label={t("settingsPage.folders.holds", { path: source.path })}
          value={kind}
          options={[
            { value: "movie", label: t("settingsPage.folders.movies") },
            { value: "tv", label: t("settingsPage.folders.tv") },
          ]}
          onChange={onKind}
        />
        <span className="flex-1" />
        <Button
          variant="ghost"
          aria-label={t("settingsPage.folders.removeFor", { path: source.path })}
          onClick={onRemove}
        >
          {t("settingsPage.folders.remove")}
        </Button>
      </div>
    </FormRow>
  );
}

/**
 * Settings, Library folders (spec `app-shell` "Settings page", `desktop-interaction` drag and
 * drop): every folder with its path, how many titles it holds, whether it is watched or offline,
 * the Movies | TV shows switch and Remove; Add folder… and folders dropped on the window.
 * Removing a folder keeps its titles in the library (offline) and offers Undo.
 */
export function FoldersSection() {
  const client = useQueryClient();
  const backend = currentSourcesBackend();
  const sources = useQuery({
    queryKey: SOURCE_LABELS_KEY,
    queryFn: () => currentSourcesBackend().list(),
  });
  const health = useQuery({
    queryKey: SOURCE_HEALTH_KEY,
    queryFn: () => currentScanBackend().sourceHealth(),
  });
  // The type just chosen for a folder, until the change is saved or refused.
  const [pending, setPending] = useState<Readonly<Record<number, SourceKind>>>({});

  const reload = useCallback(async () => {
    await Promise.all([
      client.invalidateQueries({ queryKey: SOURCE_LABELS_KEY }),
      client.invalidateQueries({ queryKey: SOURCE_HEALTH_KEY }),
      refreshLibrary(client),
    ]);
  }, [client]);

  const addPaths = useCallback(
    async (paths: readonly string[]) => {
      let added = 0;
      for (const path of paths) {
        try {
          const found = await currentOnboardingBackend().analyze(path);
          // A folder with both kinds starts as movies; the switch on its row changes it.
          const kind: SourceKind = found.suggested === "tv" ? "tv" : "movie";
          await currentSourcesBackend().add(kind, path);
          added += 1;
        } catch (error) {
          showToast({ tone: "error", message: messageOf(error) });
        }
      }
      if (added === 0) return;
      await reload();
      showToast({ message: t("settingsPage.folders.added", { count: added }) });
      // A new folder is scanned at once, so its titles appear without another click.
      if (currentScanBackend().available) void runRescan();
    },
    [reload],
  );

  const over = useFolderDrop((paths) => {
    void addPaths(paths);
  });

  const browse = () => {
    void currentOnboardingBackend()
      .pickFolders()
      .then((paths) => (paths.length > 0 ? addPaths(paths) : undefined))
      .catch(() => undefined);
  };

  const changeKind = async (source: LibrarySource, kind: SourceKind) => {
    setPending((current) => ({ ...current, [source.id]: kind }));
    try {
      await backend.setKind(source.id, kind);
      await reload();
      // The folder is read again as the other type; its titles stay in the library.
      if (currentScanBackend().available) void runRescan();
    } catch (error) {
      showToast({ tone: "error", message: messageOf(error) });
    } finally {
      setPending((current) =>
        Object.fromEntries(Object.entries(current).filter(([id]) => Number(id) !== source.id)),
      );
    }
  };

  const remove = async (source: LibrarySource) => {
    try {
      const removed = await backend.remove(source.id);
      await reload();
      showToast({
        message: t("settingsPage.folders.removed", {
          path: source.path,
          count: removed.kept_items,
        }),
        undo: async () => {
          await currentSourcesBackend().add(source.kind, source.path);
          await reload();
        },
      });
    } catch (error) {
      showToast({ tone: "error", message: messageOf(error) });
    }
  };

  const list = [...(sources.data ?? [])].sort(
    (a, b) => (a.kind === b.kind ? 0 : a.kind === "movie" ? -1 : 1) || a.position - b.position,
  );
  const healthOf = new Map((health.data ?? []).map((row) => [row.id, row]));

  return (
    <SettingsSection id="folders" title={t("settingsPage.folders.title")}>
      {sources.isPending ? (
        <div aria-busy="true" className="px-4 py-3">
          <Skeleton shape="line" />
        </div>
      ) : (
        list.map((source) => (
          <FolderRow
            key={source.id}
            source={source}
            kind={pending[source.id] ?? source.kind}
            health={healthOf.get(source.id)}
            onKind={(kind) => {
              if (kind !== (pending[source.id] ?? source.kind)) void changeKind(source, kind);
            }}
            onRemove={() => {
              void remove(source);
            }}
          />
        ))
      )}
      <FormRow>
        <div
          data-drop={over ? "over" : undefined}
          className={cx(
            "flex items-center gap-3 rounded-(--r-6)",
            over && "bg-gold-tint outline-1 outline-gold-tint-edge",
          )}
        >
          <Button icon={Plus} onClick={browse}>
            {t("settingsPage.folders.add")}
          </Button>
          <span className="text-(length:--fs-12) text-text-muted">
            {t("settingsPage.folders.drop")}
          </span>
        </div>
      </FormRow>
    </SettingsSection>
  );
}
