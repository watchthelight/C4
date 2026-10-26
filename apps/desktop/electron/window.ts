// <wtl> project: c4 | file: apps/desktop/electron/window.ts | author: watchthelight </wtl>
// The main window: the size, floor and background the Tauri window had, shown once the page has
// painted; no menu bar, no navigation away from the page, no popups.

import { BrowserWindow } from "electron";
import { log } from "./log.js";

export interface WindowOptions {
  url: string;
  preload: string;
  devtools: boolean;
}

export function createMainWindow(o: WindowOptions): BrowserWindow {
  const win = new BrowserWindow({
    width: 1280,
    height: 800,
    minWidth: 800,
    minHeight: 500,
    title: "c4",
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
    log("warn", "the page asked for a window; refused", { url });
    return { action: "deny" };
  });
  win.webContents.on("will-navigate", (event, target) => {
    if (target !== o.url) {
      log("warn", "the page asked to navigate; refused", { target });
      event.preventDefault();
    }
  });
  if (o.devtools) {
    win.webContents.openDevTools({ mode: "detach" });
  }
  void win.loadURL(o.url);
  return win;
}
