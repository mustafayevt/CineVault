#!/usr/bin/env node
/**
 * Screenshot check of UI routes.
 *
 * Starts the Vite dev server with the synthetic backend (or reuses one that already answers on the
 * URL), opens each route in headless Chrome with a throwaway profile, and captures it in dark and
 * light at 1440x900 and 1366x768. It fails (exit 1) on console errors, uncaught exceptions or
 * horizontal overflow. PNGs go to the git-ignored `ui/.shots/`.
 *
 * Usage (from ui/):
 *   bun run shots -- --routes /library,/movies/1,/gallery
 *
 * In Git Bash on Windows, set MSYS_NO_PATHCONV=1 first (`MSYS_NO_PATHCONV=1 bun run shots -- ...`):
 * the shell rewrites an argument that starts with "/" into a Git folder path. The script undoes
 * the common case for --routes, but selectors in --steps and other values can still be mangled.
 *   bun run shots -- --routes /gallery --themes dark --sizes 1440x900
 *
 * Options:
 *   --routes  comma-separated routes (default "/"); `route@@step|step` runs those steps on that
 *             route only (as for --steps), as in '/library@@[aria-label="Table view"]'
 *   --all     every route of the app (the synthetic fixtures' ids), instead of --routes, with the
 *             Torrent search notice already accepted so the page itself is captured
 *   --themes  comma-separated, any of dark,light (default both)
 *   --sizes   comma-separated WxH (default 1440x900,1366x768)
 *   --url     an already running dev server (default: start one on a free port)
 *   --out     output folder (default .shots)
 *   --wait    extra milliseconds to wait after load (default 700)
 *   --full    capture the whole scrollable page, not only the first screen
 *   --steps   steps separated by "|", run in order after the page has loaded: a CSS selector is
 *             clicked, `focus:<selector>` focuses it and `key:<Space|Escape|Enter|Delete|Arrow...>` presses
 *             a key, with modifiers as in `key:Alt+ArrowLeft` (open and pin a pane, for example:
 *             '[aria-label="Activity"]|[aria-label^="Pin Activity"]'; open the Inspector with
 *             'focus:[data-card-index="0"]|key:Space')
 *   --reduced emulate prefers-reduced-motion: reduce; files get a "-reduced" suffix
 *   --scale   the device pixel ratio (default 1), as in '--scale 2' for a high-density screen;
 *             files get an "@<scale>x" suffix
 *   --visible comma-separated CSS selectors that must be on screen without scrolling (inside the
 *             window and inside the scrolling page): each one that is not fails the check, as in
 *             '--visible [data-fact="watched"]' for the key facts of a detail page at 1366x768
 *   --scroll  a CSS selector to scroll to the middle of the screen before the capture (the first
 *             match in each theme panel when it is on a long page such as /gallery)
 *
 * Chrome is started with `--headless=new --user-data-dir=<temp dir> --no-first-run
 * --disable-extensions` and stopped by its process ID. Set CHROME to the executable when it is not
 * in a standard location. Never point this at your own browser profile.
 */
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { createServer } from "node:net";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const uiRoot = resolve(here, "..");

const args = new Map();
for (let i = 2; i < process.argv.length; i += 1) {
  const name = process.argv[i]?.replace(/^--/, "");
  if (name !== undefined && name !== "" && process.argv[i]?.startsWith("--")) {
    const next = process.argv[i + 1];
    if (next === undefined || next.startsWith("--")) args.set(name, "");
    else args.set(name, process.argv[(i += 1)] ?? "");
  }
}

/**
 * Git Bash on Windows rewrites an argument like `/movies` into `C:/Program Files/Git/movies`
 * before node sees it; undo that so `bun run shots -- --routes /movies` works in any shell.
 */
function normaliseRoute(route) {
  const unmangled = route.replace(/^[A-Za-z]:\/Program Files(?: \(x86\))?\/Git/i, "");
  if (unmangled === "") return "/";
  return unmangled.startsWith("/") ? unmangled : `/${unmangled}`;
}

/**
 * Every route of the app against the synthetic backend (`--all`): ids come from the fixtures. An
 * entry is a route or `route@@step|step` (the steps of `--steps`, run for that route only; the
 * steps also name its files).
 */
const ALL_ROUTES = [
  "/library",
  "/library@@[aria-label='Table view']",
  "/library@@[aria-label='Library'] [data-index='2']",
  "/movies/1",
  "/movies/1/edit",
  "/tv/1",
  "/tv/1/edit",
  "/tv/1/season/2",
  "/tv/1/episode/101",
  "/scrape",
  "/scrape/match/1",
  "/artwork?ids=1",
  "/artwork?ids=1%2C2%2C3&type=fanart",
  "/rename",
  "/drives",
  "/torrents",
  "/torrents?title=movie:1",
  "/torrents?title=show:1&season=1&episode=2",
  "/history",
  "/settings",
  "/welcome",
  "/gallery",
  "/no-such-page",
];

/** `route@@step|step` as `{ route, steps, tag }`; the tag keeps the files of one route's variants apart. */
function parseRoute(entry) {
  const [route = "/", stepText = ""] = entry.trim().split("@@");
  const own = stepText
    .split("|")
    .map((step) => step.trim())
    .filter(Boolean);
  const tag = own.length === 0 ? "" : `-${slug(own.join(" ")).slice(0, 24)}`;
  return { route: normaliseRoute(route), steps: own, tag };
}

const routes = (args.has("all") ? ALL_ROUTES : (args.get("routes") ?? "/").split(","))
  .filter((entry) => entry.trim() !== "")
  .map(parseRoute);
const themes = (args.get("themes") ?? "dark,light").split(",").map((theme) => theme.trim());
const sizes = (args.get("sizes") ?? "1440x900,1366x768").split(",").map((size) => {
  const [width, height] = size.trim().split("x").map(Number);
  if (!width || !height) throw new Error(`Bad size: ${size}`);
  return { width, height };
});
/** Settings stored in the browser before the page loads: `--all` and a /torrents route accept the Torrent notice. */
const seedSettings =
  args.has("all") || routes.some((entry) => entry.route.startsWith("/torrents"))
    ? { "torrents.notice-accepted": true }
    : {};
const outDir = resolve(uiRoot, args.get("out") ?? ".shots");
const settleMs = Number.parseInt(args.get("wait") ?? "700", 10);
const full = args.has("full");
const reduced = args.has("reduced");
const scale = Number.parseFloat(args.get("scale") ?? "1") || 1;
const scrollTo = args.get("scroll") ?? "";
const mustSee = (args.get("visible") ?? "")
  .split(",")
  .map((selector) => selector.trim())
  .filter(Boolean);

/** Runs inside the page: the selectors of `--visible` that are not wholly in view. */
function hiddenScript(selectors) {
  return `(() => {
    const hidden = [];
    const view = { bottom: window.innerHeight, right: window.innerWidth };
    for (const selector of ${JSON.stringify(selectors)}) {
      const el = document.querySelector(selector);
      if (el === null) { hidden.push(selector + " (not found)"); continue; }
      const rect = el.getBoundingClientRect();
      let bottom = view.bottom;
      for (let p = el.parentElement; p !== null; p = p.parentElement) {
        const style = getComputedStyle(p);
        if (/(auto|scroll|hidden)/.test(style.overflowY)) bottom = Math.min(bottom, p.getBoundingClientRect().bottom);
      }
      if (rect.height === 0 || rect.top < 0 || rect.bottom > bottom + 0.5 || rect.right > view.right + 0.5) {
        hidden.push(selector + " (bottom " + Math.round(rect.bottom) + " > " + Math.round(bottom) + ")");
      }
    }
    return hidden;
  })()`;
}
const steps = (args.get("steps") ?? "")
  .split("|")
  .map((step) => step.trim())
  .filter(Boolean);

/** Keys a `key:<name>` step can press, as the DevTools protocol wants them. */
const KEYS = {
  Space: { key: " ", code: "Space", text: " ", windowsVirtualKeyCode: 32 },
  Escape: { key: "Escape", code: "Escape", windowsVirtualKeyCode: 27 },
  Enter: { key: "Enter", code: "Enter", windowsVirtualKeyCode: 13 },
  Delete: { key: "Delete", code: "Delete", windowsVirtualKeyCode: 46 },
  ArrowLeft: { key: "ArrowLeft", code: "ArrowLeft", windowsVirtualKeyCode: 37 },
  ArrowUp: { key: "ArrowUp", code: "ArrowUp", windowsVirtualKeyCode: 38 },
  ArrowRight: { key: "ArrowRight", code: "ArrowRight", windowsVirtualKeyCode: 39 },
  ArrowDown: { key: "ArrowDown", code: "ArrowDown", windowsVirtualKeyCode: 40 },
};

function findChrome() {
  const candidates = [
    process.env.CHROME,
    "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe",
    "C:\\Program Files (x86)\\Google\\Chrome\\Application\\chrome.exe",
    "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome",
    "/usr/bin/google-chrome",
    "/usr/bin/chromium",
  ];
  const found = candidates.find((path) => path && existsSync(path));
  if (!found) throw new Error("Chrome not found; set the CHROME environment variable.");
  return found;
}

function freePort() {
  return new Promise((resolvePort, reject) => {
    const server = createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => {
      const { port } = server.address();
      server.close(() => resolvePort(port));
    });
  });
}

async function waitFor(fn, what, timeoutMs = 30000) {
  const start = Date.now();
  for (;;) {
    try {
      const value = await fn();
      if (value) return value;
    } catch {
      // not ready yet
    }
    if (Date.now() - start > timeoutMs) throw new Error(`Timed out waiting for ${what}`);
    await new Promise((r) => setTimeout(r, 150));
  }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function slug(route) {
  const text = route
    .replace(/^\/+/, "")
    .replace(/[^a-zA-Z0-9]+/g, "-")
    .replace(/-+$/, "");
  return text === "" ? "home" : text;
}

const children = [];
function stop(child) {
  if (child.exitCode !== null || child.pid === undefined) return;
  if (process.platform === "win32") {
    // Stop only this process tree, by its ID.
    spawn("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill("SIGTERM");
  }
}

async function startDevServer() {
  const explicit = args.get("url");
  if (explicit) {
    await waitFor(() => fetch(explicit).then((r) => r.ok), "the dev server");
    return explicit.replace(/\/$/, "");
  }
  const port = await freePort();
  const vite = join(uiRoot, "node_modules", "vite", "bin", "vite.js");
  const child = spawn(
    process.execPath,
    [vite, "--port", String(port), "--strictPort", "--host", "127.0.0.1"],
    { cwd: uiRoot, stdio: "ignore" },
  );
  children.push(child);
  const url = `http://127.0.0.1:${port}`;
  await waitFor(() => fetch(url).then((r) => r.ok), "the dev server");
  return url;
}

function openCdp(wsUrl) {
  return new Promise((resolveOpen, reject) => {
    const ws = new WebSocket(wsUrl);
    let nextId = 0;
    const pending = new Map();
    const listeners = [];
    ws.onmessage = (message) => {
      const data = JSON.parse(message.data);
      if (data.id !== undefined && pending.has(data.id)) {
        const { resolve: ok, reject: fail } = pending.get(data.id);
        pending.delete(data.id);
        if (data.error) fail(new Error(data.error.message));
        else ok(data.result);
      } else {
        for (const listener of listeners) listener(data);
      }
    };
    ws.onerror = reject;
    ws.onopen = () =>
      resolveOpen({
        send: (method, params = {}) =>
          new Promise((ok, fail) => {
            const id = ++nextId;
            pending.set(id, { resolve: ok, reject: fail });
            ws.send(JSON.stringify({ id, method, params }));
          }),
        on: (listener) => listeners.push(listener),
        close: () => ws.close(),
      });
  });
}

/** Runs inside the page after load: horizontal overflow of the window and of the routed content. */
const OVERFLOW_SCRIPT = `(() => {
  const root = document.documentElement;
  const main = document.querySelector('[data-route-content]');
  const problems = [];
  if (root.scrollWidth > root.clientWidth + 1) problems.push('window scrollWidth ' + root.scrollWidth + ' > ' + root.clientWidth);
  if (main && main.scrollWidth > main.clientWidth + 1) problems.push('main scrollWidth ' + main.scrollWidth + ' > ' + main.clientWidth);
  return problems;
})()`;

const results = [];
let failed = false;
let profile = null;

try {
  mkdirSync(outDir, { recursive: true });
  const base = await startDevServer();

  profile = mkdtempSync(join(tmpdir(), "cv-shots-"));
  const debugPort = await freePort();
  const chrome = spawn(
    findChrome(),
    [
      "--headless=new",
      "--disable-gpu",
      `--remote-debugging-port=${debugPort}`,
      `--user-data-dir=${profile}`,
      "--no-first-run",
      "--disable-extensions",
      "about:blank",
    ],
    { stdio: "ignore" },
  );
  children.push(chrome);

  const target = await waitFor(async () => {
    const list = await (await fetch(`http://127.0.0.1:${debugPort}/json`)).json();
    return list.find((entry) => entry.type === "page");
  }, "Chrome");
  const cdp = await openCdp(target.webSocketDebuggerUrl);

  let consoleErrors = [];
  cdp.on((event) => {
    if (event.method === "Runtime.exceptionThrown") {
      const detail = event.params.exceptionDetails;
      consoleErrors.push(detail.exception?.description ?? detail.text);
    } else if (event.method === "Runtime.consoleAPICalled" && event.params.type === "error") {
      consoleErrors.push(
        event.params.args.map((arg) => arg.value ?? arg.description ?? "").join(" "),
      );
    } else if (event.method === "Log.entryAdded" && event.params.entry.level === "error") {
      // A missing favicon is not a UI error, and a plain browser cannot load the app's own cv://
      // image protocol (the synthetic backend still asks for those images).
      const url = event.params.entry.url ?? "";
      if (!/favicon/.test(url) && !url.startsWith("cv:"))
        consoleErrors.push(event.params.entry.text);
    }
  });
  await cdp.send("Page.enable");
  await cdp.send("Runtime.enable");
  await cdp.send("Log.enable");
  if (Object.keys(seedSettings).length > 0) {
    // The settings live in localStorage in a plain browser; set them before the app reads them.
    await cdp.send("Page.addScriptToEvaluateOnNewDocument", {
      source: `try { const key = "cinevault.settings.v1"; const now = JSON.parse(localStorage.getItem(key) || "{}"); localStorage.setItem(key, JSON.stringify({ ...now, ...${JSON.stringify(seedSettings)} })); } catch (error) { void error; }`,
    });
  }

  for (const { route, steps: routeSteps, tag } of routes) {
    for (const theme of themes) {
      for (const { width, height } of sizes) {
        consoleErrors = [];
        await cdp.send("Emulation.setDeviceMetricsOverride", {
          width,
          height,
          deviceScaleFactor: scale,
          mobile: false,
        });
        // A headless page is not focused, so :focus would never match; emulate a focused page.
        await cdp.send("Emulation.setFocusEmulationEnabled", { enabled: true });
        await cdp.send("Emulation.setEmulatedMedia", {
          features: [
            { name: "prefers-color-scheme", value: theme },
            { name: "prefers-reduced-motion", value: reduced ? "reduce" : "no-preference" },
          ],
        });
        await cdp.send("Page.navigate", { url: `${base}${route}` });
        await waitFor(async () => {
          const state = await cdp.send("Runtime.evaluate", {
            expression:
              "document.readyState === 'complete' && !!document.querySelector('#root > *')",
            returnByValue: true,
          });
          return state.result.value;
        }, `${route} to render`);
        // The app applies its own theme setting; the check wants each theme, so set it directly.
        await cdp.send("Runtime.evaluate", {
          expression: `document.documentElement.dataset.theme = ${JSON.stringify(theme)}; document.fonts.ready.then(() => true)`,
          awaitPromise: true,
        });
        await sleep(settleMs);
        for (const step of [...steps, ...routeSteps]) {
          if (step.startsWith("key:")) {
            // `key:Alt+ArrowLeft`: modifiers (Alt, Ctrl, Meta, Shift) come before the key.
            const parts = step.slice(4).split("+");
            const name = parts.at(-1) ?? "";
            // A single letter or digit is typed as itself.
            const key =
              KEYS[name] ??
              (/^[A-Za-z0-9]$/.test(name)
                ? {
                    key: name,
                    code: /^[A-Za-z]$/.test(name) ? `Key${name.toUpperCase()}` : `Digit${name}`,
                    text: name,
                    windowsVirtualKeyCode: name.toUpperCase().charCodeAt(0),
                  }
                : undefined);
            if (key === undefined) throw new Error(`step ${step}: unknown key`);
            const bits = { Alt: 1, Ctrl: 2, Meta: 4, Shift: 8 };
            const modifiers = parts.slice(0, -1).reduce((sum, name) => sum + (bits[name] ?? 0), 0);
            await cdp.send("Input.dispatchKeyEvent", { type: "keyDown", modifiers, ...key });
            await cdp.send("Input.dispatchKeyEvent", { type: "keyUp", modifiers, ...key });
            await sleep(400);
            continue;
          }
          if (step.startsWith("eval:")) {
            // `eval:<js>` runs a script in the page and prints what it returns (for debugging).
            const out = await cdp.send("Runtime.evaluate", {
              expression: step.slice(5),
              returnByValue: true,
              awaitPromise: true,
            });
            console.log(`eval: ${JSON.stringify(out.result.value)}`);
            continue;
          }
          const focus = step.startsWith("focus:");
          const selector = focus ? step.slice(6) : step;
          const done = await cdp.send("Runtime.evaluate", {
            expression: `(() => { const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return false; ${focus ? "el.focus();" : "el.click();"} return true; })()`,
            returnByValue: true,
          });
          if (!done.result.value) throw new Error(`step ${step} matched nothing on ${route}`);
          await sleep(300);
        }
        const overflow = await cdp.send("Runtime.evaluate", {
          expression: OVERFLOW_SCRIPT,
          returnByValue: true,
        });
        const unseen =
          mustSee.length === 0
            ? []
            : (
                await cdp.send("Runtime.evaluate", {
                  expression: hiddenScript(mustSee),
                  returnByValue: true,
                })
              ).result.value;
        if (scrollTo !== "" && !full) {
          await cdp.send("Runtime.evaluate", {
            expression: `(() => {
              const scope = document.querySelector('[data-theme="' + ${JSON.stringify(theme)} + '"][aria-label]') ?? document;
              const el = scope.querySelector(${JSON.stringify(scrollTo)}) ?? document.querySelector(${JSON.stringify(scrollTo)});
              if (el) el.scrollIntoView({ block: 'center' });
            })()`,
          });
          await sleep(200);
        }
        let shot;
        if (full) {
          // The shell scrolls inside <main>, which also contains its size (container query), so
          // growing the page alone leaves it at the window's height: give <main> its scroll
          // height, then let the frame and the sidebar follow.
          const size = await cdp.send("Runtime.evaluate", {
            expression: `(() => {
              const main = document.querySelector('[data-route-content]');
              const style = document.createElement('style');
              style.textContent = 'html,body,#root{height:auto!important;overflow:visible!important} #root>div{height:auto!important}';
              document.head.append(style);
              if (main) {
                // Panes that scroll inside the page (Settings' section column) grow too; a very
                // tall one (a virtual list) is left alone.
                for (const el of main.querySelectorAll('*')) {
                  const overflowY = getComputedStyle(el).overflowY;
                  if ((overflowY === 'auto' || overflowY === 'scroll') && el.scrollHeight > el.clientHeight + 1 && el.scrollHeight <= 6000) {
                    el.style.cssText += ';overflow:visible;flex:none;height:' + el.scrollHeight + 'px;max-height:none';
                  }
                }
                const head = main.getBoundingClientRect().top + window.scrollY;
                main.style.cssText += ';overflow:visible;flex:none;height:' + main.scrollHeight + 'px';
                const frame = document.querySelector('[data-testid="app-frame"]');
                if (frame) frame.style.height = Math.ceil(main.scrollHeight + head) + 'px';
              }
              return Math.ceil(document.documentElement.scrollHeight);
            })()`,
            returnByValue: true,
          });
          await sleep(200);
          shot = await cdp.send("Page.captureScreenshot", {
            format: "png",
            captureBeyondViewport: true,
            clip: { x: 0, y: 0, width, height: Math.max(height, size.result.value), scale: 1 },
          });
        } else {
          shot = await cdp.send("Page.captureScreenshot", { format: "png" });
        }
        const density = scale === 1 ? "" : `@${scale}x`;
        const file = `${slug(route)}${tag}-${theme}-${width}x${height}${density}${reduced ? "-reduced" : ""}.png`;
        writeFileSync(join(outDir, file), Buffer.from(shot.data, "base64"));
        const problems = [
          ...consoleErrors.map((text) => `console error: ${text}`),
          ...overflow.result.value.map((text) => `overflow: ${text}`),
          ...unseen.map((text) => `not visible on open: ${text}`),
        ];
        if (problems.length > 0) failed = true;
        results.push({ file, problems });
      }
    }
  }
  cdp.close();
} catch (error) {
  failed = true;
  console.error(error instanceof Error ? error.message : error);
} finally {
  for (const child of children) stop(child);
  if (profile !== null) {
    // Chrome holds its profile files until it has exited; removing them is best effort.
    for (let attempt = 0; attempt < 5; attempt += 1) {
      await sleep(800);
      try {
        rmSync(profile, { recursive: true, force: true });
        break;
      } catch {
        // still in use; try again
      }
    }
  }
}

for (const { file, problems } of results) {
  console.log(`${problems.length === 0 ? "ok  " : "FAIL"} ${file}`);
  for (const problem of problems) console.log(`       ${problem}`);
}
console.log(`${results.length} screenshot(s) in ${outDir}`);
process.exit(failed ? 1 : 0);
