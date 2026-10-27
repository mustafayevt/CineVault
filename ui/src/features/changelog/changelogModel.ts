/**
 * Reads CHANGELOG.md (Keep a Changelog layout) as the app bundles it. `scripts/changelog.mjs`
 * reads the same layout for the release script and the website.
 *
 *   ## [1.1.0] - unreleased        a version section: `### Added` groups of `- ` bullets
 *   ## [1.0.0] - 2026-10-27
 *   ## Earlier versions            a plain section: a paragraph
 */

export interface ChangelogSection {
  /** The heading text: the version number, or the title of a plain section. */
  title: string;
  /** `x.y.z` for a version section, `null` for a plain one. */
  version: string | null;
  /** `YYYY-MM-DD`, or `null` when the heading has none. */
  date: string | null;
  /** True while the heading says "unreleased". */
  unreleased: boolean;
  /** The section's Markdown, without its heading. */
  body: string;
}

const VERSION_HEADING = /^##\s+\[(\d+\.\d+\.\d+)\](?:\s+-\s+(.+?))?\s*$/;
const PLAIN_HEADING = /^##\s+(.+?)\s*$/;

/** The sections of a changelog in file order; the title line and intro before them are dropped. */
export function parseChangelog(text: string): ChangelogSection[] {
  const sections: { section: Omit<ChangelogSection, "body">; lines: string[] }[] = [];
  for (const line of text.replace(/\r\n?/g, "\n").split("\n")) {
    const version = VERSION_HEADING.exec(line);
    const plain = version === null ? PLAIN_HEADING.exec(line) : null;
    if (version !== null) {
      const date = version[2]?.trim() ?? null;
      const unreleased = date !== null && /^unreleased$/i.test(date);
      sections.push({
        section: {
          title: version[1] ?? "",
          version: version[1] ?? null,
          date: unreleased ? null : date,
          unreleased,
        },
        lines: [],
      });
    } else if (plain !== null) {
      sections.push({
        section: { title: plain[1] ?? "", version: null, date: null, unreleased: false },
        lines: [],
      });
    } else {
      sections[sections.length - 1]?.lines.push(line);
    }
  }
  return sections.map(({ section, lines }) => ({ ...section, body: lines.join("\n").trim() }));
}

/** The section of one version, or `null`. */
export function sectionOf(
  sections: readonly ChangelogSection[],
  version: string,
): ChangelogSection | null {
  return sections.find((section) => section.version === version) ?? null;
}

function parts(version: string): [number, number, number] | null {
  const match = /^(\d+)\.(\d+)\.(\d+)/.exec(version);
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3])];
}

/**
 * Negative when `a` is older than `b`, positive when newer, 0 when equal; `null` when either is
 * not a version number.
 */
export function compareVersions(a: string, b: string): number | null {
  const left = parts(a);
  const right = parts(b);
  if (left === null || right === null) return null;
  return left[0] - right[0] || left[1] - right[1] || left[2] - right[2];
}
