// Reads CHANGELOG.md (Keep a Changelog layout) and turns it into what the release script and the
// website need. The app reads the same file with its own small reader (ui/src/features/changelog).
//
//   node scripts/changelog.mjs --self-test
//   node scripts/changelog.mjs --notes <x.y.z>      prints the notes of one version
//
// The layout: an optional intro, then `## [x.y.z] - YYYY-MM-DD` (or `- unreleased`) sections,
// each with optional `### Added` / `### Changed` / `### Fixed` groups of `- ` bullets, then
// optional plain `## Title` sections (for example "Earlier versions") with a paragraph.
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const changelogPath = resolve(dirname(fileURLToPath(import.meta.url)), "..", "CHANGELOG.md");

const VERSION_HEADING = /^##\s+\[(\d+\.\d+\.\d+)\](?:\s+-\s+(.+?))?\s*$/;
const SECTION_HEADING = /^##\s+(.+?)\s*$/;

/**
 * The sections of a changelog: `{ title, version, date, body }`, in file order. `version` and
 * `date` are null for a plain section; `date` is "unreleased" for the section still in progress.
 * `intro` is the text before the first section.
 */
export function parseChangelog(text) {
  const lines = String(text).replace(/\r\n?/g, "\n").split("\n");
  const sections = [];
  const intro = [];
  let current = null;
  for (const line of lines) {
    const version = VERSION_HEADING.exec(line);
    const plain = version ? null : SECTION_HEADING.exec(line);
    if (version) {
      current = { title: `${version[1]}`, version: version[1], date: version[2] ?? null, lines: [] };
      sections.push(current);
    } else if (plain && !line.startsWith("###")) {
      current = { title: plain[1], version: null, date: null, lines: [] };
      sections.push(current);
    } else if (current) {
      current.lines.push(line);
    } else if (!/^#\s/.test(line)) {
      intro.push(line);
    }
  }
  return {
    intro: intro.join("\n").trim(),
    sections: sections.map(({ lines: body, ...rest }) => ({ ...rest, body: body.join("\n").trim() })),
  };
}

/** The section of one version, or null. */
export function sectionFor(text, version) {
  return parseChangelog(text).sections.find((section) => section.version === version) ?? null;
}

/** The notes of a version as Markdown (its groups and bullets), or null when there are none. */
export function notesFor(text, version) {
  const section = sectionFor(text, version);
  return section && section.body !== "" ? section.body : null;
}

/** Reads the repository's CHANGELOG.md; null when it is missing. */
export function readChangelog(path = changelogPath) {
  try {
    return readFileSync(path, "utf8");
  } catch {
    return null;
  }
}

const escapeHtml = (text) =>
  text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;").replace(/"/g, "&quot;");

/**
 * One line of Markdown as safe HTML: text is escaped, `**bold**`, `` `code` `` and
 * `[text](https://...)` links are kept. Links other than https are left as text.
 */
export function inlineHtml(text) {
  const parts = [];
  const pattern = /\*\*(.+?)\*\*|`([^`]+)`|\[([^\]]+)\]\((https:\/\/[^\s)]+)\)/g;
  let last = 0;
  for (const match of text.matchAll(pattern)) {
    parts.push(escapeHtml(text.slice(last, match.index)));
    if (match[1] !== undefined) parts.push(`<strong>${escapeHtml(match[1])}</strong>`);
    else if (match[2] !== undefined) parts.push(`<code>${escapeHtml(match[2])}</code>`);
    else parts.push(`<a href="${escapeHtml(match[4])}">${escapeHtml(match[3])}</a>`);
    last = match.index + match[0].length;
  }
  parts.push(escapeHtml(text.slice(last)));
  return parts.join("");
}

/** A section body as HTML: `###` groups become <h3>, bullets a <ul>, other lines paragraphs. */
export function bodyHtml(body) {
  const out = [];
  let list = null;
  let paragraph = [];
  const closeParagraph = () => {
    if (paragraph.length) out.push(`<p>${inlineHtml(paragraph.join(" "))}</p>`);
    paragraph = [];
  };
  const closeList = () => {
    if (list) out.push(`<ul>\n${list.map((item) => `<li>${inlineHtml(item)}</li>`).join("\n")}\n</ul>`);
    list = null;
  };
  for (const raw of body.split("\n")) {
    const line = raw.trimEnd();
    const heading = /^#{3,6}\s+(.*)$/.exec(line);
    const bullet = /^\s*[-*+]\s+(.*)$/.exec(line);
    if (line.trim() === "") {
      closeParagraph();
      closeList();
    } else if (heading) {
      closeParagraph();
      closeList();
      out.push(`<h3>${inlineHtml(heading[1])}</h3>`);
    } else if (bullet) {
      closeParagraph();
      (list ??= []).push(bullet[1].trim());
    } else if (list && /^\s+\S/.test(raw)) {
      list[list.length - 1] += ` ${line.trim()}`;
    } else {
      closeList();
      paragraph.push(line.trim());
    }
  }
  closeParagraph();
  closeList();
  return out.join("\n");
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const sample = [
    "# Changelog",
    "",
    "Intro line.",
    "",
    "## [1.1.0] - unreleased",
    "",
    "### Added",
    "",
    "- Spotlight **search**.",
    "",
    "## [1.0.0] - 2026-10-27",
    "",
    "### Fixed",
    "- One <b>thing</b>,",
    "  on two lines.",
    "",
    "## Earlier versions",
    "",
    "See the [releases](https://example.test/r) and [bad](javascript:alert(1)).",
  ].join("\r\n");
  const parsed = parseChangelog(sample);
  assert(parsed.intro === "Intro line.", "intro");
  assert(parsed.sections.map((s) => s.version).join() === "1.1.0,1.0.0,", "sections in order");
  assert(parsed.sections[0].date === "unreleased" && parsed.sections[1].date === "2026-10-27", "dates");
  assert(notesFor(sample, "1.1.0") === "### Added\n\n- Spotlight **search**.", "notes of one version");
  assert(notesFor(sample, "9.9.9") === null, "no section, no notes");
  assert(parsed.sections[2].title === "Earlier versions", "plain section title");
  const html = bodyHtml(parsed.sections[1].body);
  assert(html === "<h3>Fixed</h3>\n<ul>\n<li>One &lt;b&gt;thing&lt;/b&gt;, on two lines.</li>\n</ul>", `html, got ${html}`);
  const links = bodyHtml(parsed.sections[2].body);
  assert(links.includes('<a href="https://example.test/r">releases</a>'), "https link kept");
  assert(!links.includes("<a href=\"javascript"), "other links stay text");
  // The real file reads: every version section has notes and an ISO date or "unreleased".
  const real = readChangelog();
  assert(real !== null, "CHANGELOG.md exists");
  const versions = parseChangelog(real).sections.filter((s) => s.version);
  assert(versions.length > 0, "CHANGELOG.md has versions");
  for (const s of versions) {
    assert(s.body !== "", `${s.version} has notes`);
    assert(s.date === "unreleased" || /^\d{4}-\d{2}-\d{2}$/.test(s.date ?? ""), `${s.version} has a date`);
  }
  console.log("changelog self-test passed");
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const args = process.argv.slice(2);
  if (args[0] === "--self-test") selfTest();
  else if (args[0] === "--notes" && args[1]) {
    const text = readChangelog();
    const notes = text === null ? null : notesFor(text, args[1].replace(/^v/, ""));
    if (notes === null) {
      console.error(`no notes for ${args[1]} in CHANGELOG.md`);
      process.exit(1);
    }
    console.log(notes);
  } else {
    console.error("usage: node scripts/changelog.mjs --self-test | --notes <x.y.z>");
    process.exit(1);
  }
}
