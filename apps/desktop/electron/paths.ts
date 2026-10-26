// <wtl> project: c4 | file: apps/desktop/electron/paths.ts | author: watchthelight </wtl>
// Where the pieces are: the core binary, the renderer build, the preload script and the C4 home.
// A packaged app finds c4-cli under its resources; a development run finds the cargo debug build,
// or whatever C4_CLI names.

import { app } from "electron";
import os from "node:os";
import path from "node:path";

const EXE = process.platform === "win32" ? ".exe" : "";

/** The C4 home as c4-core resolves it: `C4_HOME`, else `~/.c4`. */
export function c4Home(): string {
  const env = process.env.C4_HOME;
  return env !== undefined && env !== "" ? env : path.join(os.homedir(), ".c4");
}

/** The core, `c4-cli`. */
export function cliPath(): string {
  const env = process.env.C4_CLI;
  if (env !== undefined && env !== "") {
    return env;
  }
  if (app.isPackaged) {
    return path.join(process.resourcesPath, "bin", `c4-cli${EXE}`);
  }
  return path.resolve(app.getAppPath(), "..", "..", "target", "debug", `c4-cli${EXE}`);
}

/** The renderer build (`vite build`), which the app's own scheme serves. */
export function rendererDir(): string {
  return path.join(app.getAppPath(), "dist");
}

/** The preload script, built beside this file. */
export function preloadPath(): string {
  return path.join(import.meta.dirname, "preload.cjs");
}

/** A Vite dev server to load instead of the build, when `C4_DEV_URL` names one. */
export function devUrl(): string | null {
  const env = process.env.C4_DEV_URL;
  return env !== undefined && env !== "" ? env : null;
}
