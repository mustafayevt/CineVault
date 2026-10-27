import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
  redirect,
} from "@tanstack/react-router";

import { ArtworkPage } from "./features/artwork/ArtworkPage";
import { validateArtworkSearch } from "./features/artwork/artworkSearch";
import { DrivesPage } from "./features/drives/DrivesPage";
import { HistoryPage } from "./features/history/HistoryPage";
import { ReleaseNotesPage } from "./features/changelog/ReleaseNotesPage";
import { LibraryPage } from "./features/library/LibraryPage";
import { MovieEditPage, ShowEditPage } from "./features/edit/EditDetailsPage";
import { MoviePage } from "./features/movie/MoviePage";
import { ShowPage } from "./features/show/ShowPage";
import { FirstRunGate } from "./features/onboarding/FirstRunGate";
import { RenamePage } from "./features/rename/RenamePage";
import { validateRenameSearch } from "./features/rename/renameSearch";
import { MatchPickerPage } from "./features/scrape/MatchPickerPage";
import { ScrapePage } from "./features/scrape/ScrapePage";
import { validateScrapeSearch } from "./features/scrape/scrapeSearch";
import { SettingsPage } from "./features/settings/SettingsPage";
import { TorrentsPage } from "./features/torrents/TorrentsPage";
import { validateTorrentsSearch } from "./features/torrents/torrentsSearch";
import { NotFoundPage } from "./routes/NotFoundPage";

// The first-run setup replaces the window until a library exists; afterwards the app shell.
const rootRoute = createRootRoute({
  component: FirstRunGate,
});

/** The library types the Library page switches between (`/library?type=`). */
export type LibraryType = "movies" | "tv" | "collections";

/** The search of `/library`: the type, and the title bar's search text. */
export interface LibrarySearch {
  type?: LibraryType;
  q?: string | number;
}

/** Reads the search of `/library`; anything damaged is dropped. */
export function validateLibrarySearch(search: Record<string, unknown>): LibrarySearch {
  return {
    ...(search.type === "movies" || search.type === "tv" || search.type === "collections"
      ? { type: search.type }
      : {}),
    ...(typeof search.q === "string" || typeof search.q === "number" ? { q: search.q } : {}),
  };
}

// ---------------------------------------------------------------------------------------------
// The pages of the new design. Each page group replaces its placeholder or the old
// page shown here in its own feature folder, so page groups never edit this list.
// ---------------------------------------------------------------------------------------------

const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  beforeLoad: () => {
    throw redirect({ to: "/library", replace: true });
  },
});

const libraryRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/library",
  validateSearch: validateLibrarySearch,
  component: LibraryPage,
});

const movieRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/movies/$movieId",
  component: MoviePage,
});

const movieEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/movies/$movieId/edit",
  component: MovieEditPage,
});

const showEditRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/tv/$showId/edit",
  component: ShowEditPage,
});

const showRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/tv/$showId",
  component: ShowPage,
});

const scrapeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scrape",
  // `?ids=1,2` limits the page to the titles the Library sent.
  validateSearch: validateScrapeSearch,
  component: ScrapePage,
});

const matchRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/scrape/match/$itemId",
  component: MatchPickerPage,
});

const artworkRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/artwork",
  // `?ids=1,2` the titles, `?type=fanart` the type it opens on, `?collection=` a collection.
  validateSearch: validateArtworkSearch,
  component: ArtworkPage,
});

const renameRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/rename",
  // `?ids=1,2` limits the page to those titles.
  validateSearch: validateRenameSearch,
  component: RenamePage,
});

const drivesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/drives",
  component: DrivesPage,
});

const torrentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/torrents",
  // `?title=movie:123` opens with that title confirmed; `&season=1&episode=2` also picks the scope.
  validateSearch: validateTorrentsSearch,
  component: TorrentsPage,
});

const historyRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/history",
  component: HistoryPage,
});

const settingsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings",
  // `?section=<id>` opens the page at that section (providers, naming, folders, torrents, appearance, updates).
  validateSearch: (search: Record<string, unknown>): { section?: string; key?: string } => ({
    ...(typeof search.section === "string" && search.section !== ""
      ? { section: search.section }
      : {}),
    // `key=<provider>` also puts the cursor in that provider's key field (Providers section).
    ...(typeof search.key === "string" && search.key !== "" ? { key: search.key } : {}),
  }),
  component: SettingsPage,
});

const releaseNotesRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/release-notes",
  component: ReleaseNotesPage,
});

const welcomeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/welcome",
  // The first-run setup is drawn by `FirstRunGate` over the whole window, not inside the shell.
  component: () => null,
});

// Development aid: the component gallery exists only in dev builds. The dynamic import sits in
// the `import.meta.env.DEV` branch, so a production bundle neither routes nor contains the page.
const devRoutes = import.meta.env.DEV
  ? [
      createRoute({
        getParentRoute: () => rootRoute,
        path: "/gallery",
        component: lazyRouteComponent(() =>
          import("./routes/GalleryPage").then((module) => ({ default: module.GalleryPage })),
        ),
      }),
    ]
  : [];

// ---------------------------------------------------------------------------------------------
// Legacy addresses of the old pages. Old addresses with a new home redirect there; the old pages
// with no new route yet stay reachable from the old screens. Each block goes when the group that
// deletes those pages runs (library 6, title pages 7, scrape and rename 8, drives 9, settings 10).
// ---------------------------------------------------------------------------------------------

const legacySeasonRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/tv/$showId/season/$seasonNumber",
  // Seasons are sections of the show page now.
  beforeLoad: ({ params }) => {
    throw redirect({ to: "/tv/$showId", params: { showId: params.showId }, replace: true });
  },
});

const legacyEpisodeRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/tv/$showId/episode/$episodeId",
  // Episodes open inline on the show page now.
  beforeLoad: ({ params }) => {
    throw redirect({ to: "/tv/$showId", params: { showId: params.showId }, replace: true });
  },
});

/** An address of the old Settings pages: it opens the section of the one Settings page that has its content. */
function settingsRedirect<Path extends string>(path: Path, section: string | undefined) {
  return createRoute({
    getParentRoute: () => rootRoute,
    path,
    beforeLoad: () => {
      throw redirect({
        to: "/settings",
        search: section === undefined ? {} : { section },
        replace: true,
      });
    },
  });
}

// Keyboard, tools, About and the old Library and Providers pages are sections of Settings now, or
// gone with their feature; links from older screens still land somewhere sensible.
const legacyKeyboardRoute = settingsRedirect("/settings/keyboard", undefined);
const legacyLibrarySourcesRoute = settingsRedirect("/settings/library", "folders");
const legacyToolsRoute = settingsRedirect("/settings/tools", undefined);
const legacyProvidersRoute = settingsRedirect("/settings/providers", "providers");
const legacyAboutRoute = settingsRedirect("/settings/about", undefined);

const routeTree = rootRoute.addChildren([
  indexRoute,
  libraryRoute,
  movieRoute,
  movieEditRoute,
  showEditRoute,
  showRoute,
  scrapeRoute,
  matchRoute,
  artworkRoute,
  renameRoute,
  drivesRoute,
  torrentsRoute,
  historyRoute,
  settingsRoute,
  releaseNotesRoute,
  welcomeRoute,
  ...devRoutes,
  legacySeasonRoute,
  legacyEpisodeRoute,
  legacyKeyboardRoute,
  legacyLibrarySourcesRoute,
  legacyToolsRoute,
  legacyProvidersRoute,
  legacyAboutRoute,
]);

export const router = createRouter({ routeTree, defaultNotFoundComponent: NotFoundPage });

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
