// <wtl> project: c4 | file: apps/desktop/e2e/app.spec.ts | author: watchthelight </wtl>
// The packaged app end to end: the window with the core from the app's own resources against the
// fixture Claude home, the fold chord and the preference it leaves behind, the sidebar moved into
// a window of its own and back across two runs, and the relay of headless commands with their
// exit codes. The app runs as an installed one does, with Chromium's
// sandbox on (Playwright's Electron launcher would add --no-sandbox on Linux by itself), so on a
// system that restricts user namespaces the binary needs what the deb's post-install script sets
// up; docs/BUILDING.md, Packaging. Nothing here touches the real Claude home: the C4 home is a
// fresh temporary directory, the Claude home is fixtures/home, and the session list command is
// replaced with one that prints an empty list.

import { spawnSync } from "node:child_process";
import type { SpawnSyncReturns } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { _electron as electron, expect, test } from "@playwright/test";
import type { ElectronApplication, Page } from "@playwright/test";

const desktop = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const fixturesHome = path.resolve(desktop, "..", "..", "fixtures", "home");

/** The unpacked app electron-builder wrote under release/, or whatever `C4_APP` names. */
function appPath(): string {
  const env = process.env.C4_APP;
  if (env !== undefined && env !== "") {
    return env;
  }
  const release = path.join(desktop, "release");
  const candidates =
    process.platform === "win32"
      ? ["win-unpacked/c4.exe"]
      : process.platform === "darwin"
        ? ["mac-arm64/c4.app/Contents/MacOS/c4", "mac/c4.app/Contents/MacOS/c4", "mac-x64/c4.app/Contents/MacOS/c4"]
        : ["linux-unpacked/c4"];
  for (const candidate of candidates) {
    const file = path.join(release, candidate);
    if (existsSync(file)) {
      return file;
    }
  }
  throw new Error(`no unpacked app under ${release}: run pnpm package:dir first, or name one with C4_APP`);
}

/** Replaces `claude agents --json`: an empty list, so no real session is ever listed. */
const emptyAgents = process.platform === "win32" ? "cmd.exe /d /c echo []" : "echo []";

/** The environment of the app under test: this process's, with C4's variables set the test's way. */
function environment(home: string): Record<string, string> {
  const env: Record<string, string> = {};
  for (const [key, value] of Object.entries(process.env)) {
    if (value !== undefined && key !== "C4_CLI" && key !== "C4_DEV_URL") {
      env[key] = value;
    }
  }
  env.C4_HOME = home;
  env.C4_CLAUDE_HOME = fixturesHome;
  env.C4_AGENTS_CMD = emptyAgents;
  env.C4_LOG = "debug";
  return env;
}

function scratchHome(): string {
  return mkdtempSync(path.join(os.tmpdir(), "c4-e2e-"));
}

function remove(dir: string): void {
  rmSync(dir, { recursive: true, force: true, maxRetries: 5 });
}

/** What a relay left behind, for the message of an exit code that is not the expected one. */
function report(result: SpawnSyncReturns<string>): string {
  const lines = [`status ${result.status}, signal ${result.signal}`];
  if (result.error !== undefined) {
    lines.push(`error: ${result.error.message}`);
  }
  if (result.stderr !== "") {
    lines.push(`stderr: ${result.stderr.slice(0, 2000)}`);
  }
  return lines.join("\n");
}

/** The app over the C4 home `home`, and the exit code of its process once it is gone. */
async function launch(home: string): Promise<{ app: ElectronApplication; exited: Promise<number | null> }> {
  const app = await electron.launch({
    executablePath: appPath(),
    args: [],
    env: environment(home),
    cwd: home,
    chromiumSandbox: true,
    timeout: 45_000,
  });
  const exited = new Promise<number | null>((resolve) => {
    app.process().once("exit", (code) => resolve(code));
  });
  return { app, exited };
}

function readPrefs<T>(home: string): T {
  return JSON.parse(readFileSync(path.join(home, "prefs.json"), "utf8")) as T;
}

/** The part of the preferences the main process writes about the sidebar's window. */
interface WindowsPrefs {
  windows: { sidebar: { detached: boolean; bounds: Record<string, unknown> | null } };
}

/** The main window and the sidebar's own, once both are open and loaded, told apart by the query string. */
async function bothWindows(app: ElectronApplication): Promise<{ main: Page; sidebar: Page }> {
  while (app.windows().length < 2) {
    await app.waitForEvent("window");
  }
  const pages = app.windows();
  for (const page of pages) {
    await page.waitForLoadState();
  }
  const sidebar = pages.find((page) => page.url().includes("window=sidebar"));
  const main = pages.find((page) => !page.url().includes("window=sidebar"));
  if (!sidebar || !main) {
    throw new Error(`expected the main window and the sidebar's, got ${pages.map((page) => page.url()).join(", ")}`);
  }
  return { main, sidebar };
}

test("the window runs the core from its resources and keeps the fold", async () => {
  const home = scratchHome();
  const { app, exited } = await launch(home);
  try {
    expect(await app.evaluate(({ app: electronApp }) => electronApp.isPackaged)).toBe(true);
    expect(await app.evaluate(() => process.env.C4_CLI ?? "")).toBe("");

    const page = await app.firstWindow();
    await expect(page).toHaveTitle("c4");
    const root = page.locator("[data-sidebar]");
    await expect(root).toHaveAttribute("data-sidebar", "full");
    const body = page.locator("body");
    await expect(body).toContainText("bridge: electron");
    await expect(body).toContainText("home from C4_CLAUDE_HOME");
    await expect(body).toContainText("nightly build");
    await expect(body).toContainText("proj-alpha");

    await page.keyboard.press("Control+Shift+KeyB");
    await expect(root).toHaveAttribute("data-sidebar", "rail");
  } finally {
    await app.close();
  }
  expect(await exited).toBe(0);

  expect(readPrefs<{ sidebar: { view: string } }>(home).sidebar.view).toBe("rail");
  remove(home);
});

test("the sidebar moves into a window of its own and comes back where it sat", async () => {
  test.setTimeout(150_000);
  const home = scratchHome();
  const first = await launch(home);
  try {
    const main = await first.app.firstWindow();
    await expect(main).toHaveTitle("c4");
    const root = main.locator("#app");
    await expect(root).toHaveAttribute("data-sidebar", "full");
    await expect(main.locator("body")).toContainText("proj-alpha");

    // the chord opens the sidebar's window; the main window shows the rail in its place
    const opened = first.app.waitForEvent("window");
    await main.keyboard.press("Control+Shift+KeyO");
    const side = await opened;
    await expect(side).toHaveURL(/window=sidebar/);
    await expect(side).toHaveTitle("c4 sessions");
    await expect(side.locator("#app")).toHaveAttribute("data-window", "sidebar");
    await expect(side.locator("body")).toContainText("proj-alpha");
    await expect(root).toHaveAttribute("data-sidebar", "away");
    await expect(root).toHaveAttribute("data-detached", "true");

    // a session chosen in the sidebar's window is the one the main window inspects
    await side.locator(".row", { hasText: "nightly build" }).click();
    await expect(main.locator(".inspector .title")).toHaveText("nightly build");
  } finally {
    await first.app.close();
  }
  expect(await first.exited).toBe(0);

  const saved = readPrefs<WindowsPrefs>(home).windows.sidebar;
  expect(saved.detached).toBe(true);
  expect(saved.bounds).not.toBeNull();
  for (const key of ["x", "y", "width", "height"]) {
    expect(typeof saved.bounds?.[key], key).toBe("number");
  }

  // the next run opens the sidebar's window again; the fold chord there puts the sidebar back
  const second = await launch(home);
  try {
    const { main, sidebar } = await bothWindows(second.app);
    const root = main.locator("#app");
    await expect(root).toHaveAttribute("data-sidebar", "away");
    await expect(sidebar.locator("body")).toContainText("proj-alpha");
    // the keys go down only: the chord's keydown closes this window, so there is no page left
    // for the keys to come up in, and a press would fail on its keyup
    await sidebar.keyboard.down("Control");
    await sidebar.keyboard.down("Shift");
    await sidebar.keyboard.down("KeyB");
    await expect.poll(() => second.app.windows().length).toBe(1);
    await expect(root).toHaveAttribute("data-sidebar", "full");
    await expect(root).toHaveAttribute("data-detached", "false");
  } finally {
    await second.app.close();
  }
  expect(await second.exited).toBe(0);
  expect(readPrefs<WindowsPrefs>(home).windows.sidebar.detached).toBe(false);
  remove(home);
});

test("a headless command runs in the core and its exit code comes back", () => {
  const home = scratchHome();
  const exe = appPath();
  const run = (args: string[]) =>
    spawnSync(exe, args, { env: environment(home), encoding: "utf8", timeout: 60_000, windowsHide: true });

  const sessions = run(["sessions", "--files-only", "--json"]);
  expect(sessions.status, report(sessions)).toBe(0);
  const doc = JSON.parse(sessions.stdout) as { home: string; sessions: { name: string | null }[] };
  expect(doc.home).toBe(fixturesHome);
  expect(doc.sessions.map((s) => s.name)).toContain("nightly build");

  const doctor = run(["doctor", "--no-commands"]);
  expect(doctor.status, report(doctor)).toBe(0);
  expect(doctor.stdout).toMatch(/sidecar: .*c4-shim(\.exe)?: present/);

  const version = run(["--version"]);
  expect(version.status, report(version)).toBe(0);
  expect(version.stdout.trim()).toMatch(/^c4 \d+\.\d+\.\d+/);

  // a switch before the command is the window's: Chromium reads it, the core never sees it
  const switched = run(["--no-sandbox", "--version"]);
  expect(switched.status, report(switched)).toBe(0);
  expect(switched.stdout.trim()).toMatch(/^c4 \d+\.\d+\.\d+/);

  const bad = run(["nonsense"]);
  expect(bad.status, report(bad)).toBe(2);
  expect(bad.stderr).toContain("unrecognized subcommand");
  remove(home);
});
