// Builds site/changelog.html (the changelog page of the landing page) from CHANGELOG.md, so the
// two never drift apart.
//
//   node scripts/site-changelog.mjs            writes site/changelog.html
//   node scripts/site-changelog.mjs --check    fails when the page is not what CHANGELOG.md gives
//   node scripts/site-changelog.mjs --self-test
//
// The page shares style.css with index.html. It needs no script.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { bodyHtml, inlineHtml, parseChangelog, readChangelog } from "./changelog.mjs";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const outputPath = resolve(root, "site", "changelog.html");

const RELEASES = "https://github.com/mustafayevt/CineVault/releases";
const SITE = "https://mustafayevt.github.io/CineVault/";
const MARK =
  '<svg class="mark" viewBox="0 0 48 48" aria-hidden="true"><path d="M35.743 6.59A21 21 0 1 0 35.743 41.41A3 3 0 0 0 32.388 36.436A15 15 0 1 1 32.388 11.564A3 3 0 0 0 35.743 6.59Z" fill="currentColor"/><path d="M28.5 15H37.5A1.5 1.5 0 0 1 39 16.5V31.5A1.5 1.5 0 0 1 37.5 33H28.5A1.5 1.5 0 0 1 27 31.5V16.5A1.5 1.5 0 0 1 28.5 15Z" class="mark-frame"/></svg>';

const longDate = (iso) => {
  const date = new Date(`${iso}T00:00:00Z`);
  if (Number.isNaN(date.getTime())) return iso;
  return new Intl.DateTimeFormat("en-GB", { day: "numeric", month: "long", year: "numeric", timeZone: "UTC" }).format(date);
};

function entryHtml(section) {
  const body = bodyHtml(section.body);
  if (section.version === null) {
    return `    <article class="log-entry">
      <div class="log-meta"><h2>${inlineHtml(section.title)}</h2></div>
      <div class="log-body">
${body}
      </div>
    </article>`;
  }
  const unreleased = /^unreleased$/i.test(section.date ?? "");
  const when = unreleased
    ? '<span class="pill">Coming next</span>'
    : section.date
      ? `<time datetime="${section.date}">${longDate(section.date)}</time>`
      : "";
  return `    <article class="log-entry" id="v${section.version}">
      <div class="log-meta"><h2>${section.version}</h2>${when}</div>
      <div class="log-body">
${body}
      </div>
    </article>`;
}

/** The whole page for the text of CHANGELOG.md. */
export function renderPage(changelogText) {
  const { sections } = parseChangelog(changelogText);
  const latest = sections.find((section) => section.version && !/^unreleased$/i.test(section.date ?? ""));
  const description = `What changed in each version of CineVault${latest ? `, from ${latest.version} on` : ""}, in plain words.`;
  return `<!doctype html>
<!-- Generated from CHANGELOG.md by scripts/site-changelog.mjs (bun run site:changelog). Do not edit by hand. -->
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>CineVault changelog</title>
<meta name="description" content="${description}">
<meta name="color-scheme" content="dark light">
<meta name="theme-color" content="#121211" media="(prefers-color-scheme: dark)">
<meta name="theme-color" content="#f2f1ee" media="(prefers-color-scheme: light)">
<link rel="icon" href="img/favicon.svg" type="image/svg+xml">
<link rel="icon" href="img/favicon.ico" sizes="any">
<link rel="apple-touch-icon" href="img/apple-touch-icon.png">
<link rel="canonical" href="${SITE}changelog.html">
<meta property="og:type" content="website">
<meta property="og:site_name" content="CineVault">
<meta property="og:title" content="CineVault changelog">
<meta property="og:description" content="${description}">
<meta property="og:url" content="${SITE}changelog.html">
<meta property="og:image" content="${SITE}img/og.png">
<meta name="twitter:card" content="summary_large_image">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="https://fonts.googleapis.com/css2?family=Archivo:wdth,wght@100..125,400..700&family=JetBrains+Mono:wght@400;500&display=swap">
<link rel="stylesheet" href="style.css">
</head>
<body>
<a class="skip" href="#main">Skip to content</a>

<header class="bar">
  <div class="wrap bar-in">
    <a class="brand" href="./" aria-label="CineVault home">
      ${MARK}
      <span>CineVault</span>
    </a>
    <nav class="bar-nav" aria-label="Page">
      <a href="./#flow">How it works</a>
      <a href="./#features">Features</a>
      <a href="./#download">Download</a>
      <a href="changelog.html" aria-current="page">Changelog</a>
      <a href="${RELEASES}">Releases</a>
    </nav>
    <a class="btn btn-sm" href="./#download">Download</a>
  </div>
</header>

<main id="main">
  <div class="wrap log">
    <header class="log-head">
      <p class="kicker">Changelog</p>
      <h1>What's new in CineVault</h1>
      <p class="section-lede">What changed in each version, newest first.</p>
    </header>
${sections.map(entryHtml).join("\n")}
  </div>
</main>

<footer class="foot">
  <div class="wrap foot-in">
    <div class="foot-brand">
      ${MARK}
      <span>CineVault</span>
    </div>
    <p class="credit">This product uses the TMDB API but is not endorsed or certified by TMDB. Ratings come from <a href="https://www.omdbapi.com/">OMDb</a> and <a href="https://mdblist.com/">MDBList</a> when you add their keys.</p>
    <p class="foot-links"><a href="changelog.html">Changelog</a><a href="${RELEASES}">Releases and release notes</a></p>
  </div>
</footer>
</body>
</html>
`;
}

function selfTest() {
  const assert = (condition, message) => {
    if (!condition) throw new Error(`self-test failed: ${message}`);
  };
  const page = renderPage(
    "# Changelog\n\n## [2.0.0] - unreleased\n\n### Added\n- <script>x</script> & **bold**.\n\n## [1.0.0] - 2026-10-27\n\nA paragraph.\n\n## Earlier versions\n\nSee [here](https://example.test).\n",
  );
  assert(page.includes('id="v2.0.0"') && page.includes('<span class="pill">Coming next</span>'), "unreleased entry");
  assert(page.includes('<time datetime="2026-10-27">27 October 2026</time>'), "dated entry");
  assert(page.includes("&lt;script&gt;x&lt;/script&gt; &amp; <strong>bold</strong>."), "text is escaped");
  assert(!page.includes("<script>"), "no script from the changelog");
  assert(page.includes('<a href="https://example.test">here</a>'), "https links kept");
  assert(page.indexOf(`id="v2.0.0"`) < page.indexOf(`id="v1.0.0"`), "file order kept");
  console.log("site-changelog self-test passed");
}

const isMain = process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url);
if (isMain) {
  const arg = process.argv[2];
  if (arg === "--self-test") selfTest();
  else {
    const text = readChangelog();
    if (text === null) {
      console.error("site:changelog: CHANGELOG.md not found");
      process.exit(1);
    }
    const page = renderPage(text);
    if (arg === "--check") {
      const current = existsSync(outputPath) ? readFileSync(outputPath, "utf8") : "";
      if (current.replace(/\r\n/g, "\n") !== page) {
        console.error("site:changelog: site/changelog.html is stale; run `bun run site:changelog`");
        process.exit(1);
      }
      console.log("site/changelog.html is current");
    } else {
      writeFileSync(outputPath, page);
      console.log(`wrote ${outputPath}`);
    }
  }
}
