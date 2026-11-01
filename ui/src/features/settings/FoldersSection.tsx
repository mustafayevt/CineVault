import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { Folder, Network, Plus } from "lucide-react";
import { useState } from "react";

import { Button } from "../../components/Button";
import { Icon } from "../../components/Icon";
import { PathText } from "../../components/PathText";
import { SectionLabel } from "../../components/SectionLabel";
import { SegmentedControl } from "../../components/SegmentedControl";
import { Skeleton } from "../../components/Skeleton";
import { t } from "../../i18n/catalog";
import { cx } from "../../lib/cx";
import { addLibraryFolders, reloadFolders } from "../library-sources/addFolders";
import { currentSourcesBackend } from "../library-sources/sourcesBackend";
import type { LibrarySource, SourceKind } from "../library-sources/sourcesBackend";
import { currentOnboardingBackend } from "../onboarding/onboardingBackend";
import { useFolderDrop } from "../onboarding/useFolderDrop";
import { runRescan } from "../scan/scanActions";
import { currentScanBackend } from "../scan/scanBackend";
import type { SourceHealth } from "../scan/scanBackend";
import { SOURCE_HEALTH_KEY, SOURCE_LABELS_KEY } from "../scan/useSourceHealth";
import { showToast } from "../toasts/toastStore";

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

const KINDS: readonly SourceKind[] = ["movie", "tv"];

/** The line under a folder: what it holds, where it is, and whether it is watched or offline. */
function Info({ source, health }: { source: LibrarySource; health: SourceHealth | undefined }) {
  if (health === undefined) return null;
  const count = health.titles;
  const tv = source.kind === "tv";
  const share = health.share;
  if (!health.online) {
    // A share that did not connect says why, and that nothing was lost.
    if (share !== null && health.reason !== null) {
      return (
        <>
          {t("settingsPage.folders.offline")} ·{" "}
          <span className="text-warn">{t(`networkShare.reason.${health.reason}`)}</span> ·{" "}
          {t(tv ? "settingsPage.folders.kept.shows" : "settingsPage.folders.kept.titles", {
            count,
          })}
        </>
      );
    }
    return t(tv ? "settingsPage.folders.offline.shows" : "settingsPage.folders.offline.titles", {
      count,
    });
  }
  const held = t(tv ? "settingsPage.folders.shows" : "settingsPage.folders.titles", { count });
  if (share !== null) {
    return (
      <>
        {held} · {t("settingsPage.folders.on")} <span className="font-mono">{share.address}</span>{" "}
        {t("settingsPage.folders.as", { user: share.user })}
      </>
    );
  }
  return `${held} · ${t("settingsPage.folders.watched")}`;
}

interface FolderRowProps {
  source: LibrarySource;
  /** The kind shown: the one just chosen while the change is being saved. */
  kind: SourceKind;
  health: SourceHealth | undefined;
  onKind: (kind: SourceKind) => void;
  onRemove: () => void;
  onConnect: (shareId: number) => void;
}

function FolderRow({ source, kind, health, onKind, onRemove, onConnect }: FolderRowProps) {
  const offline = health !== undefined && !health.online;
  const share = health?.share ?? null;
  return (
    <div
      data-source={source.id}
      className={cx(
        "grid grid-cols-[minmax(0,1fr)_auto] items-center gap-4 border-t border-solid border-line-row px-4 py-[11px] first:border-t-0",
        offline && "text-text-disabled",
      )}
    >
      <div className="flex min-w-0 items-center gap-3">
        <span
          className={cx(
            "flex size-7 shrink-0 items-center justify-center rounded-(--r-6) bg-surface-raised",
            offline ? "text-text-disabled" : "text-text-2",
          )}
        >
          <Icon icon={share === null ? Folder : Network} />
        </span>
        <div className="min-w-0 flex-1">
          <PathText
            path={source.path}
            className={cx("font-mono text-(length:--fs-12)", !offline && "text-text")}
          />
          <div className={cx("min-h-4 text-(length:--fs-12)", !offline && "text-text-muted")}>
            <Info source={source} health={health} />
          </div>
        </div>
      </div>
      <div className="flex items-center gap-2">
        {offline && share !== null ? (
          <Button
            aria-label={t("settingsPage.folders.connectFor", { address: share.address })}
            onClick={() => {
              onConnect(share.id);
            }}
          >
            {t("settingsPage.folders.connect")}
          </Button>
        ) : null}
        <SegmentedControl<SourceKind>
          aria-label={t("settingsPage.folders.holds", { path: source.path })}
          value={kind}
          options={[
            { value: "movie", label: t("settingsPage.folders.movies") },
            { value: "tv", label: t("settingsPage.folders.tv") },
          ]}
          onChange={onKind}
        />
        <Button
          variant="ghost"
          aria-label={t("settingsPage.folders.removeFor", { path: source.path })}
          onClick={onRemove}
        >
          {t("settingsPage.folders.remove")}
        </Button>
      </div>
    </div>
  );
}

interface AddRowProps {
  kind: SourceKind;
  over: boolean;
  onBrowse: () => void;
  onShare: () => void;
}

/** Under a library's folders: Add folder… and Network share…, and the hint that folders can be dropped. */
function AddRow({ kind, over, onBrowse, onShare }: AddRowProps) {
  return (
    <div
      data-drop={over ? "over" : undefined}
      data-add={kind}
      className={cx(
        "flex items-center gap-2 border-t border-solid border-line-row px-4 py-2.5 first:border-t-0",
        over && "bg-gold-tint outline-1 -outline-offset-1 outline-gold-tint-edge",
      )}
    >
      <Button icon={Plus} onClick={onBrowse}>
        {t("settingsPage.folders.add")}
      </Button>
      <Button icon={Network} onClick={onShare}>
        {t("settingsPage.folders.addShare")}
      </Button>
      <span className="ml-1 text-(length:--fs-12) text-text-muted">
        {t("settingsPage.folders.drop")}
      </span>
    </div>
  );
}

/**
 * Settings, Library folders (spec `app-shell` "Settings page", `desktop-interaction` drag and
 * drop, `network-shares`): each library, Movies and TV shows, lists its folders with their path,
 * how many titles they hold, whether they are watched or offline (a folder on a network share
 * says which share and why it is offline, with Connect…), the Movies | TV shows switch and Remove;
 * under each library are Add folder… and Network share…, and folders can be dropped on the
 * window. Removing a folder keeps its titles in the library (offline) and offers Undo.
 */
export function FoldersSection() {
  const client = useQueryClient();
  const navigate = useNavigate();
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

  // Dropped folders take the type they look like.
  const over = useFolderDrop((paths) => {
    void addLibraryFolders(client, paths, null);
  });

  const browse = (kind: SourceKind) => {
    void currentOnboardingBackend()
      .pickFolders()
      .then((paths) => (paths.length > 0 ? addLibraryFolders(client, paths, kind) : undefined))
      .catch(() => undefined);
  };

  const changeKind = async (source: LibrarySource, kind: SourceKind) => {
    setPending((current) => ({ ...current, [source.id]: kind }));
    try {
      await backend.setKind(source.id, kind);
      await reloadFolders(client);
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
      await reloadFolders(client);
      showToast({
        message: t("settingsPage.folders.removed", {
          path: source.path,
          count: removed.kept_items,
        }),
        undo: async () => {
          await currentSourcesBackend().add(source.kind, source.path);
          await reloadFolders(client);
        },
      });
    } catch (error) {
      showToast({ tone: "error", message: messageOf(error) });
    }
  };

  const list = [...(sources.data ?? [])].sort((a, b) => a.position - b.position);
  const healthOf = new Map((health.data ?? []).map((row) => [row.id, row]));

  return (
    <section id="folders" aria-labelledby="folders-title" data-settings-section="folders">
      <h2 id="folders-title" className="m-0 mb-2.5 text-(length:--fs-16) font-(weight:--fw-700)">
        {t("settingsPage.folders.title")}
      </h2>
      {KINDS.map((kind, index) => (
        <div key={kind} role="group" aria-labelledby={`folders-${kind}`}>
          <SectionLabel
            as="h3"
            id={`folders-${kind}`}
            className={index === 0 ? "mt-3.5 mb-2" : "mt-5 mb-2"}
          >
            {t(kind === "tv" ? "settingsPage.folders.tv" : "settingsPage.folders.movies")}
          </SectionLabel>
          <div className="flex flex-col overflow-hidden rounded-(--r-10) border border-solid border-line-section bg-surface-card">
            {sources.isPending ? (
              <div aria-busy="true" className="px-4 py-3">
                <Skeleton shape="line" />
              </div>
            ) : (
              list
                .filter((source) => (pending[source.id] ?? source.kind) === kind)
                .map((source) => (
                  <FolderRow
                    key={source.id}
                    source={source}
                    kind={pending[source.id] ?? source.kind}
                    health={healthOf.get(source.id)}
                    onKind={(next) => {
                      if (next !== (pending[source.id] ?? source.kind))
                        void changeKind(source, next);
                    }}
                    onRemove={() => {
                      void remove(source);
                    }}
                    onConnect={(share) => {
                      void navigate({ to: "/network-share", search: { share } });
                    }}
                  />
                ))
            )}
            <AddRow
              kind={kind}
              over={over}
              onBrowse={() => {
                browse(kind);
              }}
              onShare={() => {
                void navigate({ to: "/network-share", search: { kind } });
              }}
            />
          </div>
        </div>
      ))}
    </section>
  );
}
