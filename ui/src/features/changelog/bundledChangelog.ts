// The changelog of the repository root, read at build time: no network is needed to show it.
import changelogText from "../../../../CHANGELOG.md?raw";
import { parseChangelog } from "./changelogModel";
import type { ChangelogSection } from "./changelogModel";

/** The sections of the bundled CHANGELOG.md, newest first as the file lists them. */
export const BUNDLED_CHANGELOG: readonly ChangelogSection[] = parseChangelog(changelogText);
