// Installs the browser transport before any module reads `isTauri()` (server mode only).
import "./lib/ipc";

// A command that finds the session gone sends the user back to sign in.
window.addEventListener("cinevault:signed-out", () => {
  if (window.location.pathname !== "/login") {
    const next = encodeURIComponent(window.location.pathname + window.location.search);
    window.location.assign(`/login?next=${next}`);
  }
});

// The stream tells the page when it could not replay what was missed; a reload rebuilds the
// job and stream state from the server instead of leaving a gap in the channel indices.
window.addEventListener("cinevault:stream-reset", () => {
  window.location.reload();
});

import { QueryClientProvider } from "@tanstack/react-query";
import { RouterProvider } from "@tanstack/react-router";
import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { FolderPickerHost } from "./features/pickers/FolderPickerHost";
import { queryClient } from "./queryClient";
import { applyBootZoom } from "./features/settings/zoom";
import { router } from "./router";
import "./styles/index.css";

// The last session's zoom, before the first screen is drawn.
applyBootZoom();

const container = document.getElementById("root");
if (!container) {
  throw new Error("Root element #root is missing from index.html");
}

createRoot(container).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <RouterProvider router={router} />
      <FolderPickerHost />
    </QueryClientProvider>
  </StrictMode>,
);
