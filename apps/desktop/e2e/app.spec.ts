// <wtl> project: c4 | file: apps/desktop/e2e/app.spec.ts | author: watchthelight </wtl>
// The packaged app end to end: the window with the core from the app's own resources against the
// fixture Claude home, the fold chord and the preference it leaves behind, and the relay of
// headless commands with their exit codes. Nothing here touches the real Claude home: the C4 home
// is a fresh temporary directory, the Claude home is fixtures/home, and the session list command
// is replaced with one that prints an empty list.

import { spawnSync } from "node:child_process";
import { existsSync, mkdtempSync, readFileSync, rmSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { _electron as electron, expect, test } from "@playwright/test";

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

test("the window runs the core from its resources and keeps the fold", async () => {
  const home = scratchHome();
  const app = await electron.launch({
    executablePath: appPath(),
    args: [],
    env: environment(home),
    cwd: home,
    timeout: 45_000,
  });
  const exited = new Promise<number | null>((resolve) => {
    app.process().once("exit", (code) => resolve(code));
  });
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

  const prefs = JSON.parse(readFileSync(path.join(home, "prefs.json"), "utf8")) as {
    sidebar: { view: string };
  };
  expect(prefs.sidebar.view).toBe("rail");
  remove(home);
});

test("a headless command runs in the core and its exit code comes back", () => {
  const home = scratchHome();
  const exe = appPath();
  const run = (args: string[]) =>
    spawnSync(exe, args, { env: environment(home), encoding: "utf8", timeout: 60_000, windowsHide: true });

  const sessions = run(["sessions", "--files-only", "--json"]);
  expect(sessions.status).toBe(0);
  const doc = JSON.parse(sessions.stdout) as { home: string; sessions: { name: string | null }[] };
  expect(doc.home).toBe(fixturesHome);
  expect(doc.sessions.map((s) => s.name)).toContain("nightly build");

  const doctor = run(["doctor", "--no-commands"]);
  expect(doctor.status).toBe(0);
  expect(doctor.stdout).toMatch(/sidecar: .*c4-shim(\.exe)?: present/);

  const version = run(["--version"]);
  expect(version.status).toBe(0);
  expect(version.stdout.trim()).toMatch(/^c4 \d+\.\d+\.\d+/);

  const bad = run(["nonsense"]);
  expect(bad.status).toBe(2);
  expect(bad.stderr).toContain("unrecognized subcommand");
  remove(home);
});
