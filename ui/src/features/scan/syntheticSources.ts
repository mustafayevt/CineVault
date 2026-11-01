import { SYNTHETIC_SHARES } from "../shares/syntheticShares";
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
    reason: null,
    share: null,
  },
  {
    id: 2,
    kind: "movie",
    path: "D:\\Archive",
    online: true,
    titles: 39,
    missing: 2,
    pending_media_info: 0,
    reason: null,
    share: null,
  },
  {
    id: 3,
    kind: "tv",
    path: "Z:\\NAS\\TV Shows",
    online: false,
    titles: 23,
    missing: 0,
    pending_media_info: 0,
    reason: null,
    share: null,
  },
];

/**
 * The folders on network shares that `?synthetic-folders` adds: one whose share answers and one
 * whose share wants its password again (the Library folders rows of the Network share spec).
 */
export const SYNTHETIC_SHARE_HEALTH: readonly SourceHealth[] = [
  {
    id: 4,
    kind: "movie",
    path: "\\\\nas\\movies\\Films",
    online: true,
    titles: 412,
    missing: 0,
    pending_media_info: 0,
    reason: null,
    share: SYNTHETIC_SHARES[0] ?? null,
  },
  {
    id: 5,
    kind: "movie",
    path: "\\\\backup\\movies\\Classics",
    online: false,
    titles: 120,
    missing: 0,
    pending_media_info: 0,
    reason: "wrong_password",
    share: SYNTHETIC_SHARES[1] ?? null,
  },
];
