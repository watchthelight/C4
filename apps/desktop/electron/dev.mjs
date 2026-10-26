// <wtl> project: c4 | file: apps/desktop/electron/dev.mjs | author: watchthelight </wtl>
// `pnpm dev`: build the main process, start Vite on 127.0.0.1:1420, open Electron against it with
// C4_DEV_URL set, and take Vite down when Electron leaves. The core is the cargo debug build of
// c4-cli unless C4_CLI names another.

import { spawn, spawnSync } from "node:child_process";
import path from "node:path";
import process from "node:process";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";
import electron from "electron";

const here = path.dirname(fileURLToPath(import.meta.url));
const app = path.resolve(here, "..");
const node = process.execPath;
const vitePort = "1420";
const url = `http://127.0.0.1:${vitePort}`;

function say(text) {
  process.stderr.write(`dev: ${text}\n`);
}

const built = spawnSync(node, [path.join(app, "node_modules", "typescript", "bin", "tsc"), "-p", "electron/tsconfig.json"], {
  cwd: app,
  stdio: "inherit",
});
if (built.status !== 0) {
  process.exit(built.status ?? 1);
}

const vite = spawn(
  node,
  [path.join(app, "node_modules", "vite", "bin", "vite.js"), "--port", vitePort, "--strictPort", "--host", "127.0.0.1"],
  { cwd: app, stdio: "inherit" },
);

let child = null;
function stop() {
  if (child !== null && child.exitCode === null) {
    child.kill();
  }
  if (vite.exitCode === null) {
    vite.kill();
  }
}
process.on("SIGINT", stop);
process.on("SIGTERM", stop);
vite.on("exit", (code) => {
  if (child === null || child.exitCode === null) {
    say(`vite exited with ${code ?? "a signal"}`);
    stop();
    process.exit(code ?? 1);
  }
});

const deadline = Date.now() + 30_000;
let up = false;
while (!up && Date.now() < deadline) {
  try {
    const res = await fetch(url);
    up = res.ok;
  } catch {
    await sleep(150);
  }
}
if (!up) {
  say(`vite did not answer on ${url} within 30 s`);
  stop();
  process.exit(1);
}

say(`vite on ${url}; starting electron`);
child = spawn(electron, [app], {
  cwd: app,
  stdio: "inherit",
  env: { ...process.env, C4_DEV_URL: url },
});
child.on("exit", (code, signal) => {
  stop();
  say(`electron left (${signal ?? `exit ${code ?? "?"}`})`);
  process.exit(code ?? 0);
});
