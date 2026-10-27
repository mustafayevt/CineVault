#!/usr/bin/env node
// Cold start of the real desktop app on a 5,000-movie library.
//
//   cargo build --release -p cinevault-app      (after `bun run build` in ui/)
//   node scripts/perf-coldstart.mjs [--runs 5] [--keep]
//
// 1. The `cold_start_5k` perf probe of cv-app builds a portable install in a temp folder (a
//    `portable` marker, a `data` folder holding a library of 5,000 scanned movies, and the
//    generated `movies` tree it was scanned from).
// 2. The release executable is copied next to the marker, and started `--runs` times, each time
//    after the previous process and its WebView2 children have exited. Edge WebView2 is started
//    with a remote debugging port (WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS), which is how the
//    clock sees the page.
// 3. The clock starts just before the process is spawned and stops when the first poster card is
//    in the DOM (the grid is on screen and scrollable). The startup scan is on, as it is for a
//    user.
//
// Windows only: it needs the WebView2 runtime and the debugging argument it understands. The
// medians and the individual runs are printed as JSON; compare them between versions on the same machine.
import { spawn, spawnSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = new Map();
for (let i = 2; i < process.argv.length; i += 1) {
  const name = process.argv[i]?.replace(/^--/, "");
  if (name === "keep") args.set(name, "true");
  else args.set(name, process.argv[(i += 1)]);
}
const runs = Number.parseInt(args.get("runs") ?? "5", 10);
const port = 9444;

if (process.platform !== "win32") {
  console.error("The cold-start measurement drives WebView2 and runs on Windows only.");
  process.exit(1);
}
const exe = join(root, "target", "release", "cinevault-app.exe");
if (!existsSync(exe)) {
  console.error(`Missing ${exe}. Run "cargo build --release -p cinevault-app" first.`);
  process.exit(1);
}

// The app allows one instance (tauri-plugin-single-instance): a second launch hands over to the
// running one and exits, so there would be nothing to time.
const tasks = spawnSync("tasklist", ["/FO", "CSV", "/NH"], { encoding: "utf8" }).stdout ?? "";
const running = [...tasks.matchAll(/^"([^"]+)"/gm)]
  .map((match) => match[1].toLowerCase())
  .filter((name) => name === "cinevault-app.exe" || name === "cinevault.exe");
if (running.length > 0) {
  console.error("CineVault is already running. Close it first: the app allows one instance.");
  process.exit(2);
}

const stage = mkdtempSync(join(tmpdir(), "cv-coldstart-"));
const install = join(stage, "install");
mkdirSync(install, { recursive: true });

console.error(`Building the 5,000-movie library in ${install} ...`);
const build = spawnSync(
  "cargo",
  [
    "test",
    "--release",
    "-p",
    "cv-app",
    "--test",
    "perf_budgets",
    "--",
    "--ignored",
    "--nocapture",
    "cold_start_5k",
  ],
  {
    cwd: root,
    env: { ...process.env, CV_PERF_PORTABLE_DIR: install },
    stdio: ["ignore", "ignore", "inherit"],
    shell: process.platform === "win32",
  },
);
if (build.status !== 0) process.exit(build.status ?? 1);

const app = join(install, "CineVault.exe");
copyFileSync(exe, app);

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

async function evaluate(url, expression) {
  const ws = new WebSocket(url);
  await new Promise((ok, fail) => {
    ws.onopen = ok;
    ws.onerror = fail;
  });
  const answer = await new Promise((ok) => {
    ws.onmessage = (m) => ok(JSON.parse(m.data));
    ws.send(
      JSON.stringify({
        id: 1,
        method: "Runtime.evaluate",
        params: { expression, returnByValue: true },
      }),
    );
  });
  ws.close();
  return answer.result?.result?.value;
}

async function oneRun() {
  const started = performance.now();
  const child = spawn(app, [], {
    cwd: install,
    env: {
      ...process.env,
      WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${port}`,
    },
    stdio: "ignore",
  });
  let exited = false;
  child.on("exit", () => {
    exited = true;
  });
  let windowAt = null;
  let readyAt = null;
  while (readyAt === null && performance.now() - started < 30000 && !exited) {
    try {
      const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
      const page = list.find((t) => t.type === "page" && t.webSocketDebuggerUrl);
      if (page) {
        windowAt ??= performance.now() - started;
        const ready = await evaluate(
          page.webSocketDebuggerUrl,
          `document.querySelectorAll('[role="gridcell"] button[data-card-index]').length`,
        );
        if (ready > 0) readyAt = performance.now() - started;
      }
    } catch {
      // the debugging port is not open yet
    }
    if (readyAt === null) await sleep(10);
  }
  // Stop the app and WebView2's processes, which belong to this install's profile.
  spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  await sleep(2000);
  return { pageTargetMs: windowAt && Math.round(windowAt), firstCardMs: readyAt && Math.round(readyAt) };
}

try {
  const results = [];
  for (let i = 0; i < runs; i += 1) results.push(await oneRun());
  const cards = results.map((r) => r.firstCardMs).filter((n) => n !== null && n !== undefined);
  cards.sort((a, b) => a - b);
  const median = cards.length ? cards[Math.floor(cards.length / 2)] : null;
  console.log(
    JSON.stringify(
      {
        when: new Date().toISOString(),
        library: 5000,
        runs: results,
        firstCardMedianMs: median,
        firstCardMaxMs: cards.length ? cards[cards.length - 1] : null,
        budgetMs: 1500,
      },
      null,
      2,
    ),
  );
} finally {
  if (!args.has("keep")) {
    try {
      rmSync(stage, { recursive: true, force: true });
    } catch {
      // Windows can hold the folder for a moment; the OS temp folder is cleaned later
    }
  }
}
