#!/usr/bin/env node
// The performance budget suite. Runs every probe that has a budget (the ceilings live in the
// probes themselves), in the release profile, one at a time, and prints each probe's own
// figures (the Rust probes print them with --nocapture).
//
//   node scripts/perf-budgets.mjs [--skip grid,thumbs] [--jobs 4]
//
// Steps: query (20k titles), rescan (5k), cold start (5k, backend part), input latency under
// load (backend, on four logical cores on Windows), the 5,000-poster thumbnail probe, and the
// poster-grid trace with the page-side cold start and input latency (needs `bun run build` in
// ui/ first). The real desktop cold start is `node scripts/perf-coldstart.mjs`.
//
// Run it on a quiet machine: nothing else compiling, no other CineVault instance.
import { spawn, spawnSync } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const root = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const args = new Map();
for (let i = 2; i < process.argv.length; i += 1) {
  args.set(process.argv[i].replace(/^--/, ""), process.argv[(i += 1)]);
}
const skip = new Set((args.get("skip") ?? "").split(",").filter(Boolean));
const env = { ...process.env, CARGO_BUILD_JOBS: args.get("jobs") ?? "4" };
const windows = process.platform === "win32";
const failures = [];

async function step(name, fn) {
  if (skip.has(name)) {
    console.log(`\n== ${name}: skipped`);
    return;
  }
  console.log(`\n== ${name}`);
  const ok = await fn();
  if (!ok) failures.push(name);
}

function cargoTest(pkg, test, extra = [], filter = []) {
  return (
    spawnSync(
      "cargo",
      [
        "test",
        "--release",
        "-p",
        pkg,
        "--test",
        test,
        "--",
        "--nocapture",
        "--test-threads=1",
        ...extra,
        ...filter,
      ],
      { cwd: root, env, stdio: "inherit", shell: windows },
    ).status === 0
  );
}

/** The path of a built integration-test executable. */
function testExecutable(pkg, test) {
  const out = spawnSync(
    "cargo",
    [
      "test",
      "--release",
      "-p",
      pkg,
      "--test",
      test,
      "--no-run",
      "--message-format=json",
    ],
    { cwd: root, env, encoding: "utf8", shell: windows, maxBuffer: 1 << 28 },
  ).stdout;
  for (const line of out.split("\n")) {
    try {
      const message = JSON.parse(line);
      if (message.executable && message.target?.name === test)
        return message.executable;
    } catch {
      // not a JSON line
    }
  }
  throw new Error(`no test executable for ${pkg}/${test}`);
}

/**
 * Starts a program with its processor affinity set to the first four logical processors (Windows).
 * PowerShell's Start-Process -Wait semantics keep the program's real exit code: the child exits
 * with it, so a failing probe fails the step (`cmd /c start /wait` drops it).
 */
function spawnPinned(command, commandArgs, options) {
  const quote = (text) => `'${text.replace(/'/g, "''")}'`;
  const commandLine = (text) =>
    /[\s"]/.test(text) ? `"${text.replace(/"/g, '\\"')}"` : text;
  const script =
    `$p = Start-Process -FilePath ${quote(command)} -ArgumentList @(${commandArgs
      .map((a) => quote(commandLine(a)))
      .join(",")}) -NoNewWindow -PassThru; ` +
    "$p.ProcessorAffinity = 15; $p.WaitForExit(); exit $p.ExitCode";
  return spawn(
    "pwsh",
    ["-NoProfile", "-NonInteractive", "-Command", script],
    options,
  );
}

await step("query", () => cargoTest("cv-db", "library_query_speed"));
await step("rescan", () => cargoTest("cv-scan", "rescan_5k"));
await step("cold-start", () =>
  cargoTest("cv-app", "perf_budgets", ["--ignored"], ["cold_start_5k"]),
);
await step("input-latency", async () => {
  const exe = testExecutable("cv-app", "perf_budgets");
  // A four-core machine: on Windows both the probe and the browser are pinned to the first four
  // logical processors, so the page competes with the backend load for the same cores.
  const pinned = (command, commandArgs, options) =>
    windows
      ? spawnPinned(command, commandArgs, options)
      : spawn(command, commandArgs, options);
  const preview = spawn(
    "bunx",
    ["vite", "preview", "--port", "5197", "--strictPort"],
    {
      cwd: join(root, "ui"),
      stdio: "ignore",
      shell: windows,
    },
  );
  let pageRun = null;
  const probe = pinned(
    exe,
    ["--ignored", "--nocapture", "input_latency_under_load"],
    {
      cwd: root,
      stdio: ["ignore", "inherit", "pipe"],
    },
  );
  probe.stderr.on("data", (chunk) => {
    process.stderr.write(chunk);
    if (pageRun === null && chunk.toString().includes("load started")) {
      pageRun = new Promise((done) => {
        const page = pinned(
          process.execPath,
          [
            "scripts/perf-grid.mjs",
            "--url",
            "http://127.0.0.1:5197",
            "--input-only",
          ],
          { cwd: join(root, "ui"), stdio: "inherit" },
        );
        page.on("exit", (code) => done(code === 0));
      });
    }
  });
  const probeOk = await new Promise((done) =>
    probe.on("exit", (code) => done(code === 0)),
  );
  const pageOk = pageRun === null ? false : await pageRun;
  if (windows)
    spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], {
      stdio: "ignore",
    });
  else preview.kill();
  return probeOk && pageOk;
});
await step("batch-match", () =>
  cargoTest("cv-app", "batch_match_1200", ["--ignored"]),
);
await step("thumbs", () =>
  cargoTest("cv-artwork", "thumb_job_5k", ["--ignored"]),
);
await step("grid", () => {
  const port = "5198";
  const ui = join(root, "ui");
  const preview = spawn(
    "bunx",
    ["vite", "preview", "--port", port, "--strictPort"],
    {
      cwd: ui,
      stdio: "ignore",
      shell: windows,
    },
  );
  try {
    spawnSync(process.execPath, ["-e", "setTimeout(() => {}, 4000)"]);
    return (
      spawnSync(
        process.execPath,
        [
          "scripts/perf-grid.mjs",
          "--url",
          `http://127.0.0.1:${port}`,
          "--out",
          "../target/perf",
        ],
        { cwd: ui, stdio: "inherit" },
      ).status === 0
    );
  } finally {
    if (windows)
      spawnSync("taskkill", ["/PID", String(preview.pid), "/T", "/F"], {
        stdio: "ignore",
      });
    else preview.kill();
  }
});

console.log(
  failures.length
    ? `\nBudget suite: FAILED in ${failures.join(", ")}`
    : "\nBudget suite: all steps passed",
);
process.exit(failures.length ? 1 : 0);
