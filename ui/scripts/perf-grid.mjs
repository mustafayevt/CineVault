#!/usr/bin/env node
/**
 * Scrolling performance trace of the poster grid on a synthetic library.
 *
 * Starts headless Chrome, opens the Library (Movies, grid) of a running dev server or preview build with N
 * generated items (`?synthetic=N`, served by the fixture backend), records a Chrome trace while a
 * script flings the grid down and back up and then jumps to distant rows, and prints a
 * summary: frame intervals (a frame that takes longer than 1.5 frames at 60 Hz counts as dropped),
 * main-thread task times from the trace, how many cards were ever mounted at once, and how many
 * frames showed a skeleton tile inside the viewport.
 *
 * Usage:
 *   bunx vite --port 5199 --strictPort &        # or `bun run build && bunx vite preview`
 *   node scripts/perf-grid.mjs [--url http://127.0.0.1:5199] [--items 20000] [--fling-px 80000]
 *                              [--out ../target/perf]
 *
 * Two more measurements ride along:
 *   - `firstCard`: milliseconds from the start of the navigation to the first poster card in the
 *     DOM, in a cold browser profile (the page part of the cold-start budget; the fixture backend
 *     answers in the page, so the desktop backend's time is not in it).
 *   - `inputUnderLoad`: after the trace, real key events are sent from the debugger every 100 ms
 *     for 8 s while the CPU is throttled 4x (a slow four-core machine), with the grid idle and
 *     while it is being flung (the worst case), and while it is flung without throttling. The
 *     Event Timing API reports each event's time from key press to next paint; budget 50 ms.
 * Pass --no-input-load to skip the second one. Pass --input-only to run only the keys-only,
 * unthrottled input measurement and print it (nothing is written): scripts/perf-budgets.mjs runs
 * it pinned to four cores while the backend load probe runs on the same four.
 *
 * The summary is written to <out>/grid-scroll-summary.json and the gzipped trace to
 * <out>/grid-scroll.trace.json.gz (open it in chrome://tracing or the DevTools Performance panel).
 * The saved trace keeps metadata and every event of 1 ms or more; pass --full-trace for all of it.
 * Set CHROME to the browser executable when it is not in a standard location.
 */
import { spawn } from "node:child_process";
import { existsSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { gzipSync } from "node:zlib";

const args = new Map();
const FLAGS = new Set(["full-trace", "no-input-load", "input-only"]);
for (let i = 2; i < process.argv.length; i += 1) {
  const name = process.argv[i]?.replace(/^--/, "");
  if (FLAGS.has(name)) args.set(name, "true");
  else args.set(name, process.argv[(i += 1)]);
}
const baseUrl = args.get("url") ?? "http://127.0.0.1:5199";
const items = Number.parseInt(args.get("items") ?? "20000", 10);
const flingPx = Number.parseInt(args.get("fling-px") ?? "80000", 10);
const outDir = resolve(args.get("out") ?? "../target/perf");
const port = 9333;

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

async function waitFor(fn, what, timeoutMs = 15000) {
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

const profile = mkdtempSync(join(tmpdir(), "cv-perf-"));
const chrome = spawn(
  findChrome(),
  [
    "--headless=new",
    "--disable-gpu",
    "--hide-scrollbars",
    "--window-size=1440,900",
    `--remote-debugging-port=${port}`,
    `--user-data-dir=${profile}`,
    "--no-first-run",
    "about:blank",
  ],
  { stdio: "ignore" },
);

/** Runs inside the page: flings the grid and records what the user would see. */
const SCROLL_SCRIPT = `(async () => {
  const scroller = document.querySelector('[data-testid="poster-scroll"]');
  if (!scroller) throw new Error('poster grid not found');
  const maxTop = () => scroller.scrollHeight - scroller.clientHeight;
  const intervals = [];
  let maxCards = 0;
  let framesWithSkeleton = 0;
  let framesMeasured = 0;
  const longTasks = [];
  try {
    new PerformanceObserver((list) => {
      for (const entry of list.getEntries()) longTasks.push(entry.duration);
    }).observe({ entryTypes: ['longtask'] });
  } catch {}

  const inViewportSkeleton = () => {
    const box = scroller.getBoundingClientRect();
    for (const cell of scroller.querySelectorAll('[data-index]:not([data-id])')) {
      const r = cell.getBoundingClientRect();
      if (r.bottom > box.top && r.top < box.bottom) return true;
    }
    return false;
  };
  const sample = () => {
    const cards = scroller.querySelectorAll('[data-index]').length;
    if (cards > maxCards) maxCards = cards;
    framesMeasured += 1;
    if (inViewportSkeleton()) framesWithSkeleton += 1;
  };

  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));

  // Phase 1: trackpad-style flings. Each one starts at 16,000 px/s (about 50 rows a second, far
  // faster than a hand flick) and decays like inertia over 10 s, covering 80,000 px (about 250 rows).
  async function fling(from, to, ms) {
    const start = performance.now();
    let last = start;
    for (;;) {
      const now = await frame();
      intervals.push(now - last);
      last = now;
      const t = Math.min(1, (now - start) / ms);
      // Quadratic ease-out: the speed starts at 2 x distance / time and falls to zero.
      const eased = 1 - Math.pow(1 - t, 2);
      scroller.scrollTop = from + (to - from) * eased;
      sample();
      if (t >= 1) return;
    }
  }
  await frame();
  const total = maxTop();
  const FLING_PX = ${flingPx};
  await fling(0, FLING_PX, 10000);
  await fling(FLING_PX, 0, 10000);

  // Phase 2: scrollbar drags to distant rows, then a moment for the pages to arrive.
  const skeletonAfterJump = [];
  for (const fraction of [0.5, 0.9, 0.2, 0.75]) {
    scroller.scrollTop = total * fraction;
    const jumpAt = performance.now();
    let previous = jumpAt;
    let filledAfter = null;
    for (let i = 0; i < 90; i += 1) {
      const now = await frame();
      intervals.push(now - previous);
      previous = now;
      sample();
      if (filledAfter === null && !inViewportSkeleton()) filledAfter = now - jumpAt;
    }
    skeletonAfterJump.push(filledAfter);
  }

  return {
    frames: intervals.length,
    intervals,
    maxCards,
    framesWithSkeleton,
    framesMeasured,
    skeletonAfterJump,
    longTasks,
    rows: Number(scroller.dataset.rows ?? 0),
    columns: Number(scroller.dataset.columns ?? 0),
  };
})()`;

/** Installed before the page loads: notes when the first poster card reaches the DOM. */
const FIRST_CARD_HOOK = `(() => {
  const check = () => {
    if (window.__firstCard !== undefined) return true;
    if (document.querySelector('[data-index][data-id] button')) {
      window.__firstCard = performance.now();
      return true;
    }
    return false;
  };
  const start = () => {
    if (check()) return;
    new MutationObserver((_, observer) => {
      if (check()) observer.disconnect();
    }).observe(document, { childList: true, subtree: true });
  };
  if (document.documentElement) start();
  else document.addEventListener('readystatechange', start, { once: true });
})()`;

/**
 * Runs inside the page: records Event Timing entries for key events and, when `fling` is set,
 * keeps the grid scrolling at the same time (the worst case: the main thread is rendering rows
 * whenever a key arrives).
 */
const inputLoadScript = (fling) => `(() => {
  const scroller = document.querySelector('[data-testid="poster-scroll"]');
  window.__eventTimings = [];
  window.__loadDone = false;
  if (window.__eventObserver) window.__eventObserver.disconnect();
  window.__eventObserver = new PerformanceObserver((list) => {
    for (const entry of list.getEntries()) {
      if (entry.name === 'keydown' || entry.name === 'keyup') {
        window.__eventTimings.push({ name: entry.name, duration: entry.duration, delay: entry.processingStart - entry.startTime, processing: entry.processingEnd - entry.processingStart });
      }
    }
  });
  window.__eventObserver.observe({ type: 'event', durationThreshold: 16, buffered: false });
  scroller.scrollTop = 0;
  scroller.focus();
  const frame = () => new Promise((resolve) => requestAnimationFrame(resolve));
  window.__load = (async () => {
    const start = performance.now();
    for (;;) {
      const now = await frame();
      const t = Math.min(1, (now - start) / 8000);
      if (${fling}) scroller.scrollTop = 40000 * (1 - Math.pow(1 - t, 2));
      if (t >= 1) break;
    }
    window.__loadDone = true;
  })();
  return true;
})()`;

/**
 * Sends a key press every 100 ms for 8 s from the debugger and returns the Event Timing figures:
 * how long each keydown and keyup took from the key press to the next paint. Event Timing only
 * lists events slower than 16 ms (its smallest threshold), so the fast ones are counted as
 * 16 ms here, which can only overstate the percentiles.
 */
async function measureInput(send, evaluate, { throttle, fling }) {
  await send("Emulation.setCPUThrottlingRate", { rate: throttle });
  await evaluate(inputLoadScript(fling));
  const keys = ["ArrowRight", "ArrowDown", "ArrowLeft", "ArrowUp"];
  let sent = 0;
  const key = (name) => ({
    key: name,
    code: name,
    windowsVirtualKeyCode: { ArrowLeft: 37, ArrowUp: 38, ArrowRight: 39, ArrowDown: 40 }[name],
  });
  while (!(await evaluate("window.__loadDone"))) {
    const name = keys[sent % keys.length];
    await send("Input.dispatchKeyEvent", { type: "rawKeyDown", ...key(name) });
    await send("Input.dispatchKeyEvent", { type: "keyUp", ...key(name) });
    sent += 1;
    await new Promise((r) => setTimeout(r, 100));
  }
  await new Promise((r) => setTimeout(r, 500));
  const timings = await evaluate("window.__eventTimings");
  await send("Emulation.setCPUThrottlingRate", { rate: 1 });
  const total = sent * 2;
  const slow = timings.map((t) => t.duration).sort((a, b) => a - b);
  const all = [...Array(Math.max(0, total - slow.length)).fill(16), ...slow];
  return {
    cpuThrottle: throttle,
    gridFlingAtTheSameTime: fling,
    keyEvents: total,
    eventsOver16ms: slow.length,
    eventsOver50ms: slow.filter((d) => d > 50).length,
    p50EventMs: Math.round(percentile(all, 50)),
    p99EventMs: Math.round(percentile(all, 99)),
    medianInputDelayMs: Math.round(
      percentile(
        timings.map((t) => t.delay).sort((a, b) => a - b),
        50,
      ),
    ),
    medianHandlerMs: Math.round(
      percentile(
        timings.map((t) => t.processing).sort((a, b) => a - b),
        50,
      ),
    ),
    maxEventMs: slow.length ? Math.round(slow[slow.length - 1]) : 16,
  };
}

async function measureInputUnderLoad(send, evaluate) {
  const run = (throttle, fling) => measureInput(send, evaluate, { throttle, fling });
  return {
    keysOnly: await run(1, false),
    keysOnlyThrottled2x: await run(2, false),
    keysOnlyThrottled4x: await run(4, false),
    keysDuringFling: await run(1, true),
    keysDuringFlingThrottled4x: await run(4, true),
  };
}

function percentile(sorted, p) {
  if (sorted.length === 0) return 0;
  return sorted[Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length))];
}

try {
  const target = await waitFor(async () => {
    const list = await (await fetch(`http://127.0.0.1:${port}/json`)).json();
    return list.find((t) => t.type === "page");
  }, "Chrome");

  const ws = new WebSocket(target.webSocketDebuggerUrl);
  await new Promise((resolveOpen, reject) => {
    ws.onopen = resolveOpen;
    ws.onerror = reject;
  });
  let nextId = 0;
  const pending = new Map();
  const events = [];
  ws.onmessage = (message) => {
    const data = JSON.parse(message.data);
    if (data.id !== undefined && pending.has(data.id)) {
      const { resolve: ok, reject } = pending.get(data.id);
      pending.delete(data.id);
      if (data.error) reject(new Error(`${data.error.message}`));
      else ok(data.result);
    } else {
      events.push(data);
    }
  };
  const send = (method, params = {}) =>
    new Promise((ok, reject) => {
      const id = ++nextId;
      pending.set(id, { resolve: ok, reject });
      ws.send(JSON.stringify({ id, method, params }));
    });

  await send("Page.enable");
  await send("Runtime.enable");
  await send("Emulation.setDeviceMetricsOverride", {
    width: 1440,
    height: 900,
    deviceScaleFactor: 1,
    mobile: false,
  });
  const evaluate = async (expression) => {
    const result = await send("Runtime.evaluate", {
      expression,
      returnByValue: true,
      awaitPromise: true,
    });
    if (result.exceptionDetails) throw new Error(JSON.stringify(result.exceptionDetails));
    return result.result.value;
  };
  await send("Page.addScriptToEvaluateOnNewDocument", { source: FIRST_CARD_HOOK });
  await send("Page.navigate", { url: `${baseUrl}/library?synthetic=${items}` });
  await waitFor(async () => {
    return (await evaluate(`window.__firstCard !== undefined`)) === true;
  }, "the grid to render");
  const firstCardMs = await evaluate(`window.__firstCard`);
  await new Promise((r) => setTimeout(r, 1000));

  main: {
    if (args.has("input-only")) {
      const keysOnly = await measureInput(send, evaluate, { throttle: 1, fling: false });
      console.log(JSON.stringify({ firstCardMs: Math.round(firstCardMs), keysOnly }, null, 2));
      ws.close();
      // The input-latency budget: a key press paints within 50 ms at the p99.
      if (keysOnly.p99EventMs >= 50) {
        console.error(`input latency p99 ${keysOnly.p99EventMs} ms, the budget is 50 ms`);
        process.exitCode = 1;
      }
      break main;
    }

    await send("Tracing.start", {
      transferMode: "ReturnAsStream",
      traceConfig: {
        includedCategories: [
          "devtools.timeline",
          "disabled-by-default-devtools.timeline",
          "v8.execute",
          "blink.user_timing",
          "toplevel",
        ],
      },
    });
    const run = await send("Runtime.evaluate", {
      expression: SCROLL_SCRIPT,
      awaitPromise: true,
      returnByValue: true,
      timeout: 120000,
    });
    if (run.exceptionDetails) throw new Error(JSON.stringify(run.exceptionDetails));
    const page = run.result.value;

    await send("Tracing.end");
    const complete = await waitFor(
      () => events.find((e) => e.method === "Tracing.tracingComplete"),
      "the trace",
      60000,
    );
    let traceText = "";
    for (;;) {
      const chunk = await send("IO.read", { handle: complete.params.stream, size: 1 << 20 });
      traceText += chunk.data;
      if (chunk.eof) break;
    }
    await send("IO.close", { handle: complete.params.stream });

    const inputUnderLoad = args.has("no-input-load")
      ? null
      : await measureInputUnderLoad(send, evaluate);

    // Main-thread work from the trace.
    const trace = JSON.parse(traceText);
    const traceEvents = Array.isArray(trace) ? trace : trace.traceEvents;
    const mainThreads = new Set(
      traceEvents
        .filter((e) => e.name === "thread_name" && e.args?.name === "CrRendererMain")
        .map((e) => `${e.pid}:${e.tid}`),
    );
    const tasks = traceEvents
      .filter((e) => e.name === "RunTask" && e.ph === "X" && mainThreads.has(`${e.pid}:${e.tid}`))
      .map((e) => e.dur / 1000)
      .sort((a, b) => a - b);

    const intervals = [...page.intervals].sort((a, b) => a - b);
    const dropped = page.intervals.filter((ms) => ms > 25).length;
    const summary = {
      when: new Date().toISOString(),
      items,
      flingPx,
      firstCardMs: Math.round(firstCardMs),
      inputUnderLoad,
      layout: { rows: page.rows, columns: page.columns },
      frames: page.frames,
      frameIntervalMs: {
        p50: +percentile(intervals, 50).toFixed(1),
        p95: +percentile(intervals, 95).toFixed(1),
        p99: +percentile(intervals, 99).toFixed(1),
        max: +Math.max(...intervals).toFixed(1),
      },
      droppedFrames: dropped,
      droppedFramesShare: +(dropped / page.frames).toFixed(4),
      mainThreadTaskMs: {
        count: tasks.length,
        p95: +percentile(tasks, 95).toFixed(2),
        p99: +percentile(tasks, 99).toFixed(2),
        max: +(tasks[tasks.length - 1] ?? 0).toFixed(2),
        over16ms: tasks.filter((ms) => ms > 16).length,
        over50ms: tasks.filter((ms) => ms > 50).length,
      },
      longTasksOver50ms: page.longTasks.length,
      maxMountedCards: page.maxCards,
      framesWithSkeletonInViewport: page.framesWithSkeleton,
      framesMeasured: page.framesMeasured,
      msUntilNoSkeletonAfterJump: page.skeletonAfterJump.map((ms) =>
        ms === null ? null : Math.round(ms),
      ),
    };

    mkdirSync(outDir, { recursive: true });
    writeFileSync(
      join(outDir, "grid-scroll-summary.json"),
      `${JSON.stringify(summary, null, 2)}\n`,
    );
    // The full trace is 8 to 11 MB compressed. Keep what the Performance panel needs to show the
    // frames and the long tasks: metadata, plus every complete event of 1 ms or more. `--full-trace`
    // keeps everything.
    const keep = args.has("full-trace")
      ? traceEvents
      : traceEvents.filter((e) => e.ph === "M" || (e.ph === "X" && e.dur >= 1000));
    writeFileSync(
      join(outDir, "grid-scroll.trace.json.gz"),
      gzipSync(JSON.stringify({ traceEvents: keep })),
    );
    console.log(JSON.stringify(summary, null, 2));
    ws.close();
  }
} finally {
  chrome.kill();
  await new Promise((r) => setTimeout(r, 500));
  try {
    rmSync(profile, { recursive: true, force: true });
  } catch {
    // the profile folder can stay locked for a moment on Windows; the OS temp folder is cleaned later
  }
}
