// <wtl> project: c4 | file: apps/desktop/electron/windows.ts | author: watchthelight </wtl>
// The main window and, when the sidebar is detached, the sidebar's own window. This controller
// opens and closes them, keeps the placement and the sidebar window's last bounds in the `windows`
// section of the preferences through the core, and reports every change of placement so the pages
// can follow. Closing the sidebar window puts the sidebar back in the main window; closing the main
// window ends the app, takes the sidebar window with it and keeps the placement for the next start.

import { EventEmitter } from "node:events";
import { BrowserWindow, screen } from "electron";
import { placeSidebar, readWindows, savedSidebarWidth, windowsSection } from "./bounds.js";
import type { Rect } from "./bounds.js";
import { EVENT } from "./ipc.js";
import { log } from "./log.js";
import type { Sidecar } from "./sidecar.js";
import { createAppWindow, sidebarUrl } from "./window.js";

export type Placement = "attached" | "detached";

/** A move or a resize of the sidebar window is written after this quiet. */
export const BOUNDS_DELAY_MS = 500;
/** A write of the placement waits this long for a core that is still starting. */
export const PREFS_WAIT_MS = 15_000;

export interface WindowsOptions {
  url: string;
  preload: string;
  devtools: boolean;
}

interface Events {
  change: [Placement];
}

export class Windows extends EventEmitter<Events> {
  private main: BrowserWindow | null = null;
  private sidebar: BrowserWindow | null = null;
  private detached = false;
  private bounds: Rect | null = null;
  private quitting = false;
  private timer: NodeJS.Timeout | null = null;
  /** Settles once the placement of the last run has been put back, or could not be. */
  ready: Promise<void> = Promise.resolve();

  constructor(
    private readonly core: Sidecar,
    private readonly o: WindowsOptions,
  ) {
    super();
  }

  get placement(): Placement {
    return this.detached ? "detached" : "attached";
  }

  get mainWindow(): BrowserWindow | null {
    return this.main;
  }

  get sidebarWindow(): BrowserWindow | null {
    return this.sidebar !== null && !this.sidebar.isDestroyed() ? this.sidebar : null;
  }

  openMain(): BrowserWindow {
    const win = createAppWindow({ kind: "main", url: this.o.url, preload: this.o.preload, devtools: this.o.devtools });
    this.main = win;
    // the main window is the app: with it gone the sidebar window goes too, and the placement
    // stays as it was for the next start
    win.on("closed", () => {
      this.main = null;
      this.quitting = true;
      this.sidebarWindow?.close();
    });
    return win;
  }

  /** Bring the main window up; a second `c4` asks for this. */
  focusMain(): void {
    const win = this.main;
    if (win === null || win.isDestroyed()) {
      return;
    }
    if (win.isMinimized()) {
      win.restore();
    }
    win.focus();
  }

  /** At startup: open the sidebar window when the last run left it detached. */
  async restore(doc: unknown): Promise<void> {
    if (readWindows(doc).sidebar.detached) {
      await this.detach(doc);
    }
  }

  /**
   * Open the sidebar window where the `windows` section of `doc` (read from the core when absent)
   * last had it, and record the placement. An open window is brought up instead.
   */
  async detach(doc?: unknown): Promise<Placement> {
    const open = this.sidebarWindow;
    if (open !== null) {
      open.focus();
      return "detached";
    }
    const prefs = doc ?? (await this.readPrefs());
    const saved = readWindows(prefs).sidebar.bounds ?? this.bounds;
    const main = this.main !== null && !this.main.isDestroyed() ? this.main.getBounds() : { x: 0, y: 0, width: 1280, height: 800 };
    const displays = screen.getAllDisplays().map((d) => d.workArea);
    const bounds = placeSidebar(saved, displays, main, savedSidebarWidth(prefs));
    const win = createAppWindow({
      kind: "sidebar",
      url: sidebarUrl(this.o.url),
      preload: this.o.preload,
      devtools: this.o.devtools,
      bounds,
    });
    this.sidebar = win;
    this.bounds = bounds;
    const moved = (): void => {
      if (!win.isDestroyed()) {
        this.bounds = win.getBounds();
        this.later();
      }
    };
    win.on("move", moved);
    win.on("resize", moved);
    // the last bounds are read while the window still exists; on the way out of the app they
    // are written at once, since nothing else will
    win.on("close", () => {
      this.cancel();
      if (!win.isDestroyed()) {
        this.bounds = win.getBounds();
      }
      if (this.quitting) {
        this.persist();
      }
    });
    // closed by its own button or through attach(): the sidebar is back in the main window, which
    // takes the focus the sidebar window had
    win.on("closed", () => {
      if (this.sidebar === win) {
        this.sidebar = null;
      }
      if (!this.quitting) {
        this.setDetached(false);
        this.focusMain();
      }
    });
    log("info", "sidebar window open", { ...bounds });
    this.setDetached(true);
    return "detached";
  }

  /** Close the sidebar window and put the sidebar back in the main window. */
  attach(): Placement {
    const win = this.sidebarWindow;
    this.setDetached(false);
    win?.close();
    return "attached";
  }

  /** Bring the sidebar window up and ask its page to focus the filter box. True when there is one. */
  focusSidebar(): boolean {
    const win = this.sidebarWindow;
    if (win === null) {
      return false;
    }
    if (win.isMinimized()) {
      win.restore();
    }
    win.focus();
    win.webContents.send(EVENT, { event: "shell", data: { kind: "focus" } });
    return true;
  }

  /** On the way out: every window closes and the placement stays as it is. */
  closeAll(): void {
    this.quitting = true;
    this.cancel();
    for (const win of BrowserWindow.getAllWindows()) {
      win.close();
    }
  }

  private setDetached(detached: boolean): void {
    if (detached === this.detached) {
      return;
    }
    this.detached = detached;
    this.persist();
    this.emit("change", this.placement);
  }

  private async readPrefs(): Promise<unknown> {
    try {
      await this.core.whenRunning(PREFS_WAIT_MS);
      return await this.core.invoke("prefs_get", null);
    } catch (error) {
      log("warn", "could not read the window placement", { error: String(error) });
      return {};
    }
  }

  private later(): void {
    this.cancel();
    this.timer = setTimeout(() => {
      this.timer = null;
      this.persist();
    }, BOUNDS_DELAY_MS);
  }

  private cancel(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }

  /** Write the placement and the bounds; a core that cannot take them is logged, not fatal. */
  private persist(): void {
    const value = windowsSection({ sidebar: { detached: this.detached, bounds: this.bounds } });
    void this.core
      .whenRunning(PREFS_WAIT_MS)
      .then(() => this.core.invoke("prefs_set", { section: "windows", value }))
      .catch((error: unknown) => {
        log("warn", "could not keep the window placement", { error: String(error) });
      });
  }
}
