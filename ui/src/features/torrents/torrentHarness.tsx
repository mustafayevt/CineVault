import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  RouterProvider,
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
} from "@tanstack/react-router";
import { render, screen } from "@testing-library/react";
import { vi } from "vitest";

import type { ProviderSearchTransport } from "../workspaces/providerEvents";
import { useSettingsStore } from "../settings/settingsStore";
import { setTorrentBackend } from "./torrentBackend";
import type { TorrentBackend, TorrentRelease } from "./torrentBackend";
import { forgetOwnJackett } from "./useOwnJackett";
import { TorrentsPage } from "./TorrentsPage";
import { createSyntheticTorrentBackend } from "./syntheticTorrents";
import type { SyntheticTorrentBackend, SyntheticTorrentOptions } from "./syntheticTorrents";
import { validateTorrentsSearch } from "./torrentsSearch";

/** What a spied transport was started with. */
export interface Started {
  query: string;
  params: Readonly<Record<string, string | number | boolean>> | undefined;
}

export interface SpiedBackend extends SyntheticTorrentBackend {
  releaseStarts: Started[];
  titleStarts: Started[];
}

/** The synthetic backend (quick delays) with every search start recorded. */
export function spiedBackend(options: SyntheticTorrentOptions = {}): SpiedBackend {
  const base = createSyntheticTorrentBackend({ speed: 0.01, ...options });
  const releaseStarts: Started[] = [];
  const titleStarts: Started[] = [];
  const wrap = <T,>(
    transport: ProviderSearchTransport<T>,
    log: Started[],
  ): ProviderSearchTransport<T> => ({
    ...transport,
    start: (request, onEvent) => {
      log.push({ query: request.query, params: request.params });
      transport.start(request, onEvent);
    },
  });
  return {
    ...base,
    releaseStarts,
    titleStarts,
    titleTransport: (onStart) => wrap(base.titleTransport(onStart), titleStarts),
    releaseTransport: (onStart) => wrap(base.releaseTransport(onStart), releaseStarts),
    openMagnet: vi.fn(base.openMagnet),
    openTorrentFile: vi.fn(base.openTorrentFile),
    openPage: vi.fn(base.openPage),
    setSource: vi.fn(base.setSource),
  };
}

/** A backend whose release search answers with exactly these releases from one source. */
export function backendWith(releases: TorrentRelease[]): SpiedBackend {
  const base = spiedBackend();
  return {
    ...base,
    releaseTransport: (onStart) => {
      const inner = base.releaseTransport(onStart);
      return {
        ...inner,
        start: (request, onEvent) => {
          onStart?.({
            sources: [{ id: "torrentio", name: "Torrentio", kind: "built-in", skipped: null }],
          });
          onEvent({ kind: "started", provider: "torrentio" });
          onEvent({ kind: "results", provider: "torrentio", items: releases });
          onEvent({ kind: "done" });
          base.releaseStarts.push({ query: request.query, params: request.params });
        },
      };
    },
  };
}

/**
 * Renders the page at `/torrents` with the given backend. The responsibility notice is accepted
 * unless `accepted` is false (the first visit).
 */
export async function renderTorrents(
  path = "/torrents",
  backend: TorrentBackend = spiedBackend(),
  accepted = true,
) {
  setTorrentBackend(backend);
  useSettingsStore.setState({
    values: accepted ? { "torrents.notice-accepted": true } : {},
    status: "ready",
  });
  forgetOwnJackett();
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: Infinity } },
  });
  const rootRoute = createRootRoute();
  const route = createRoute({
    getParentRoute: () => rootRoute,
    path: "/torrents",
    validateSearch: validateTorrentsSearch,
    component: TorrentsPage,
  });
  const router = createRouter({
    routeTree: rootRoute.addChildren([route]),
    history: createMemoryHistory({ initialEntries: [path] }),
  });
  await router.load();
  const view = render(
    <QueryClientProvider client={client}>
      <RouterProvider router={router} />
    </QueryClientProvider>,
  );
  await screen.findByTestId("torrents-page");
  return { ...view, router, backend, client };
}

/** A release for tests that need exact rows. */
export function release(overrides: Partial<TorrentRelease> = {}): TorrentRelease {
  const id = overrides.id ?? "a".repeat(40);
  return {
    id,
    absorbs: [],
    name: "Dune.2021.2160p.WEB-DL.DV.Atmos-GRP",
    size: 20_000_000_000,
    seeders: 40,
    leechers: 3,
    published: Date.UTC(2026, 9, 20),
    sources: ["torrentio"],
    pages: [],
    magnet: `magnet:?xt=urn:btih:${id}`,
    torrentUrl: null,
    infoHash: id,
    resolution: "2160p",
    source: "WEB-DL",
    codec: "HEVC",
    hdr: "DV",
    audio: "Atmos",
    pack: null,
    flags: [],
    upgrade: null,
    matching: true,
    ...overrides,
  };
}
