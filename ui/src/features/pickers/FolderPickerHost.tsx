import { useCallback, useEffect, useRef, useState } from "react";

import { isServer } from "../../lib/transport";
import { PICK_EVENT } from "./folderPickerRequest";
import type { PickRequest } from "./folderPickerRequest";

interface FolderEntry {
  name: string;
  path: string;
}

interface Listing {
  path: string;
  parent: string | null;
  roots: string[];
  folders: FolderEntry[];
}

function csrf(): string {
  const value = (window as unknown as Record<string, unknown>).__CINEVAULT_CSRF__;
  return typeof value === "string" ? value : "";
}

async function readListing(path: string): Promise<Listing> {
  const query = path === "" ? "" : `?path=${encodeURIComponent(path)}`;
  const response = await fetch(`/api/fs/list${query}`, { credentials: "same-origin" });
  const body = (await response.json()) as Listing & { message?: string };
  if (!response.ok) throw new Error(body.message ?? "That folder cannot be read.");
  return body;
}

/**
 * The server folder browser, shown where the desktop app opens the system picker. It resolves the
 * request the picker backend dispatched, so the calling page keeps its normal promise.
 *
 * It is a page over the shell, not a dialog: the same surface tokens and focus ring as the rest of
 * the app, with the browser's own Esc handled here.
 */
export function FolderPickerHost() {
  const [request, setRequest] = useState<PickRequest | null>(null);
  const [listing, setListing] = useState<Listing | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [newName, setNewName] = useState<string | null>(null);
  const [pathInput, setPathInput] = useState("");
  const nameInput = useRef<HTMLInputElement>(null);

  const load = useCallback(async (path: string) => {
    setError(null);
    try {
      const value = await readListing(path);
      setListing(value);
      setPathInput(value.path);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }, []);

  useEffect(() => {
    if (!isServer()) return;
    const onPick = (event: Event) => {
      const detail = (event as CustomEvent<PickRequest>).detail;
      setRequest(detail);
      setNewName(null);
      void load("");
    };
    window.addEventListener(PICK_EVENT, onPick);
    return () => window.removeEventListener(PICK_EVENT, onPick);
  }, [load]);

  useEffect(() => {
    if (newName !== null) nameInput.current?.focus();
  }, [newName]);

  useEffect(() => {
    if (request === null) return;
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        event.preventDefault();
        request.resolve([]);
        setRequest(null);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [request]);

  if (request === null) return null;

  const close = (paths: string[]) => {
    request.resolve(paths);
    setRequest(null);
  };

  const makeFolder = async () => {
    if (newName === null || listing === null || newName.trim() === "") return;
    setBusy(true);
    setError(null);
    try {
      const separator = listing.path.includes("\\") ? "\\" : "/";
      const path = `${listing.path.replace(/[\\/]+$/, "")}${separator}${newName.trim()}`;
      const response = await fetch("/api/fs/mkdir", {
        method: "POST",
        credentials: "same-origin",
        headers: { "content-type": "application/json", "x-cinevault-csrf": csrf() },
        body: JSON.stringify({ path }),
      });
      const body = (await response.json()) as { message?: string };
      if (!response.ok) throw new Error(body.message ?? "The folder could not be made.");
      setNewName(null);
      await load(listing.path);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setBusy(false);
    }
  };

  return (
    <section
      aria-label={request.title ?? "Choose a folder"}
      style={{
        position: "fixed",
        inset: 0,
        zIndex: 60,
        display: "flex",
        flexDirection: "column",
        background: "var(--surface-window)",
        color: "var(--text)",
      }}
    >
      <header
        style={{
          display: "flex",
          alignItems: "center",
          gap: 12,
          padding: "14px 20px",
          borderBottom: "1px solid var(--line-section)",
        }}
      >
        <strong style={{ fontSize: 15 }}>{request.title ?? "Choose a folder"}</strong>
        <span style={{ color: "var(--text-2)", fontSize: 12 }}>
          Folders on the CineVault server
        </span>
        <button
          type="button"
          onClick={() => close([])}
          style={{
            marginLeft: "auto",
            background: "transparent",
            border: 0,
            color: "var(--text-2)",
            cursor: "pointer",
            font: "inherit",
          }}
        >
          Cancel
        </button>
      </header>

      <div style={{ display: "flex", gap: 8, padding: "12px 20px" }}>
        <button
          type="button"
          disabled={listing?.parent === null || listing === null}
          onClick={() => listing?.parent && void load(listing.parent)}
          style={{ font: "inherit", cursor: "pointer" }}
        >
          Up
        </button>
        <input
          value={pathInput}
          onChange={(event) => setPathInput(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") void load(pathInput.trim());
          }}
          aria-label="Folder path on the server"
          spellCheck={false}
          style={{
            flex: 1,
            minWidth: 0,
            background: "var(--surface-well)",
            border: "1px solid var(--line-section)",
            borderRadius: 8,
            padding: "6px 10px",
            color: "var(--text)",
            fontSize: 12,
            fontFamily: "inherit",
          }}
        />
        <button
          type="button"
          disabled={busy || listing === null}
          onClick={() => setNewName("")}
          style={{ font: "inherit", cursor: "pointer" }}
        >
          New folder
        </button>
      </div>

      {listing !== null && listing.roots.length > 0 ? (
        <div
          style={{
            display: "flex",
            flexWrap: "wrap",
            gap: 8,
            padding: "0 20px 10px",
          }}
        >
          {listing.roots.map((root) => (
            <button
              key={root}
              type="button"
              onClick={() => void load(root)}
              style={{
                font: "inherit",
                fontSize: 12,
                padding: "4px 8px",
                borderRadius: 6,
                border: "1px solid var(--line-section)",
                background: "var(--surface-card)",
                color: "var(--text-2)",
                cursor: "pointer",
              }}
            >
              {root}
            </button>
          ))}
        </div>
      ) : null}

      {newName !== null ? (
        <div style={{ display: "flex", gap: 8, padding: "0 20px 12px" }}>
          <input
            ref={nameInput}
            value={newName}
            onChange={(event) => setNewName(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") void makeFolder();
            }}
            placeholder="Folder name"
            style={{
              flex: 1,
              padding: "8px 10px",
              borderRadius: 8,
              border: "1px solid var(--line-section)",
              background: "var(--surface-well)",
              color: "var(--text)",
              font: "inherit",
            }}
          />
          <button type="button" disabled={busy} onClick={() => void makeFolder()}>
            Make
          </button>
        </div>
      ) : null}

      {error !== null ? (
        <p role="alert" style={{ color: "var(--danger)", padding: "0 20px" }}>
          {error}
        </p>
      ) : null}

      <div style={{ flex: 1, overflowY: "auto", padding: "0 12px" }}>
        {listing?.folders.map((folder) => (
          <button
            key={folder.path}
            type="button"
            onClick={() => void load(folder.path)}
            style={{
              display: "flex",
              width: "100%",
              alignItems: "center",
              gap: 10,
              padding: "9px 10px",
              border: 0,
              borderRadius: 8,
              background: "transparent",
              color: "var(--text)",
              cursor: "pointer",
              textAlign: "left",
              font: "inherit",
            }}
            onMouseEnter={(event) => {
              event.currentTarget.style.background = "var(--surface-floating-hover)";
            }}
            onMouseLeave={(event) => {
              event.currentTarget.style.background = "transparent";
            }}
          >
            <span aria-hidden>▸</span>
            <span>{folder.name}</span>
          </button>
        ))}
        {listing !== null && listing.folders.length === 0 ? (
          <p style={{ color: "var(--text-muted)", padding: "12px 10px" }}>No folders here.</p>
        ) : null}
      </div>

      <footer
        style={{
          display: "flex",
          justifyContent: "flex-end",
          gap: 10,
          padding: "14px 20px",
          borderTop: "1px solid var(--line-section)",
        }}
      >
        <button type="button" onClick={() => close([])} style={{ font: "inherit" }}>
          Cancel
        </button>
        <button
          type="button"
          disabled={listing === null}
          onClick={() => listing && close([listing.path])}
          style={{
            font: "inherit",
            fontWeight: 600,
            background: "var(--gold)",
            color: "var(--surface-window)",
            border: 0,
            borderRadius: 8,
            padding: "8px 14px",
            cursor: "pointer",
          }}
        >
          Choose this folder
        </button>
      </footer>
    </section>
  );
}
