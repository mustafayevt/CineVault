import type { SourceHealth } from "./scanBackend";

/**
 * The folders a plain browser shows in the sidebar (development and the screenshot check): two
 * movie folders whose default labels ("Movies", "Archive") are the Source labels of the fixture
 * library's rows, and a drive that is not connected. Labels are unique within a library type, so
 * the fixtures never show two folders named alike.
 */
export const SYNTHETIC_SOURCE_HEALTH: readonly SourceHealth[] = [
  {
    id: 1,
    kind: "movie",
    path: "C:\\Users\\alex\\Media\\Movies",
    online: true,
    titles: 142,
    missing: 0,
    pending_media_info: 0,
  },
  {
    id: 2,
    kind: "movie",
    path: "D:\\Archive",
    online: true,
    titles: 39,
    missing: 2,
    pending_media_info: 0,
  },
  {
    id: 3,
    kind: "tv",
    path: "Z:\\NAS\\TV Shows",
    online: false,
    titles: 23,
    missing: 0,
    pending_media_info: 0,
  },
];
