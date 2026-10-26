// <wtl> project: c4 | file: apps/desktop/electron/window.ts | author: watchthelight </wtl>
// The windows: the main one, 1280 by 800 with an 800 by 500 floor, and the sidebar's own window
// when the sidebar is detached, which loads the same page with `?window=sidebar` where the last run
// left it and keeps a 220 by 240 floor. Both carry the app's icon and background, show once the
// page has painted, hide the menu bar, navigate nowhere else and open no popups.

import { BrowserWindow } from "electron";
import type { Rect } from "./bounds.js";
import { SIDEBAR_WINDOW_MIN } from "./bounds.js";
import { log } from "./log.js";
import { iconPath } from "./paths.js";

export type WindowKind = "main" | "sidebar";

export interface WindowOptions {
  kind: WindowKind;
  url: string;
  preload: string;
  devtools: boolean;
  /** Where the window opens; without it the window takes its default size where the system puts it. */
  bounds?: Rect;
}

export const MAIN_SIZE = { width: 1280, height: 800 } as const;
export const MAIN_MIN = { width: 800, height: 500 } as const;
export const SIDEBAR_SIZE = { width: 320, height: 720 } as const;

/** The page's URL for the sidebar window: the same page, told which window it is in. */
export function sidebarUrl(url: string): string {
  const u = new URL(url);
  u.searchParams.set("window", "sidebar");
  return u.toString();
}

export function createAppWindow(o: WindowOptions): BrowserWindow {
  const main = o.kind === "main";
  const size = main ? MAIN_SIZE : SIDEBAR_SIZE;
  const min = main ? MAIN_MIN : SIDEBAR_WINDOW_MIN;
  const win = new BrowserWindow({
    width: o.bounds?.width ?? size.width,
    height: o.bounds?.height ?? size.height,
    ...(o.bounds === undefined ? {} : { x: o.bounds.x, y: o.bounds.y }),
    minWidth: min.width,
    minHeight: min.height,
    title: main ? "c4" : "c4 sessions",
    icon: iconPath(),
    backgroundColor: "#08080c",
    show: false,
    autoHideMenuBar: true,
    webPreferences: {
      preload: o.preload,
      contextIsolation: true,
      sandbox: true,
      nodeIntegration: false,
      webSecurity: true,
      spellcheck: false,
      devTools: o.devtools,
    },
  });
  win.setMenuBarVisibility(false);
  win.once("ready-to-show", () => {
    win.show();
  });
  win.webContents.setWindowOpenHandler(({ url }) => {
    log("warn", "the page asked for a window; refused", { url, window: o.kind });
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (event, target) => {
    if (target !== o.url) {
      log("warn", "the page asked to navigate; refused", { target, window: o.kind });
      event.preventDefault();
    }
  });
  if (o.devtools) {
    win.webContents.openDevTools({ mode: "detach" });
  }
  void win.loadURL(o.url);
  return win;
}
