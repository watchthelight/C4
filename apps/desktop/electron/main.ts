// <wtl> project: c4 | file: apps/desktop/electron/main.ts | author: watchthelight </wtl>
// The `c4` executable. With arguments it is the command line (`c4 sessions`, `c4 doctor`,
// `c4 install`, ...): the arguments go to c4-cli in this console, its exit code comes back, and no
// window opens. Without any it is the window: one instance, the core started as a child, the page
// served from the app's own scheme, the sidebar's own window put back where the last run left it,
// the core stopped before the process leaves.

import { spawnSync } from "node:child_process";
import { existsSync } from "node:fs";
import path from "node:path";
import { Menu, app, dialog, session } from "electron";
import { relayArgs } from "./cli.js";
import { START_WAIT_MS, installIpc } from "./ipc.js";
import { log } from "./log.js";
import { c4Home, cliPath, devUrl, preloadPath, rendererDir } from "./paths.js";
import { ORIGIN, registerScheme, serveRenderer } from "./protocol.js";
import { Sidecar } from "./sidecar.js";
import { Windows } from "./windows.js";

const relay = relayArgs(process.argv, process.defaultApp === true);
if (relay !== null) {
  runCommand(relay);
} else {
  runWindow();
}

/** `c4 <args>`: c4-cli runs in this console with the same arguments and the same exit code. */
function runCommand(args: string[]): void {
  const exe = cliPath();
  const result = spawnSync(exe, args, { stdio: "inherit", windowsHide: true });
  if (result.error !== undefined) {
    process.stderr.write(`c4: could not run ${exe}: ${result.error.message}\n`);
    app.exit(127);
    return;
  }
  app.exit(result.status ?? 1);
}

function runWindow(): void {
  if (!app.requestSingleInstanceLock()) {
    app.exit(0);
    return;
  }
  app.setName("c4");
  app.setAppUserModelId("org.watchthelight.c4");
  app.setPath("userData", path.join(c4Home(), "electron"));
  registerScheme();

  const exe = cliPath();
  if (!existsSync(exe)) {
    const why = `${exe} does not exist. Build it with cargo build -p c4-cli, or name another with C4_CLI.`;
    log("error", why);
    dialog.showErrorBox("c4: the core is missing", why);
    app.exit(1);
    return;
  }
  const core = new Sidecar(exe);
  const dev = devUrl();
  const url = dev ?? `${ORIGIN}/index.html`;
  const windows = new Windows(core, { url, preload: preloadPath(), devtools: process.env.C4_DEVTOOLS === "1" });
  let stopped = false;
  let quitting = false;

  app.on("second-instance", () => {
    windows.focusMain();
  });
  app.on("window-all-closed", () => {
    app.quit();
  });
  // the windows go first, so what a page still holds (a preference written moments ago) reaches
  // the core; then the core: stdin closed, the ingest endpoint freed; then the process may leave
  app.on("before-quit", (event) => {
    if (stopped) {
      return;
    }
    event.preventDefault();
    if (quitting) {
      return;
    }
    quitting = true;
    windows.closeAll();
    void core.stop().then(() => {
      stopped = true;
      app.quit();
    });
  });
  core.on("fatal", (why) => {
    dialog.showErrorBox("c4: the core cannot be used", why);
    stopped = true;
    app.exit(1);
  });
  // Chromium replaces a GPU process that crashed or hung and falls back to software rendering on
  // its own; until then the window paints nothing, so the log says when it happened and why
  app.on("child-process-gone", (_event, details) => {
    if (details.type === "GPU") {
      log("warn", "the GPU process is gone; Chromium falls back to software rendering", {
        reason: details.reason,
        exitCode: details.exitCode,
      });
    }
  });

  void app.whenReady().then(() => {
    // no menu bar; macOS keeps its application menu, which carries Cmd+Q and the edit shortcuts
    if (process.platform === "darwin") {
      Menu.setApplicationMenu(
        Menu.buildFromTemplate([{ role: "appMenu" }, { role: "editMenu" }, { role: "windowMenu" }]),
      );
    } else {
      Menu.setApplicationMenu(null);
    }
    session.defaultSession.setPermissionRequestHandler((_contents, _permission, callback) => {
      callback(false);
    });
    session.defaultSession.setPermissionCheckHandler(() => false);
    serveRenderer(rendererDir());
    installIpc(core, dev === null ? [ORIGIN] : [ORIGIN, dev], windows);
    core.start();
    windows.openMain();
    // the sidebar window of the last run comes back once the core can say where it was; the main
    // window does not wait for that
    windows.ready = core
      .whenRunning(START_WAIT_MS)
      .then(() => core.invoke("prefs_get", null))
      .then((doc) => windows.restore(doc))
      .catch((error: unknown) => {
        log("warn", "could not put the window layout back", { error: String(error) });
      });
    log("info", "window open", { url, core: exe, userData: app.getPath("userData") });
  });
}
