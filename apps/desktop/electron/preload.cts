// <wtl> project: c4 | file: apps/desktop/electron/preload.cts | author: watchthelight </wtl>
// The only door between the page and the main process: `window.c4` with a request function, an
// event subscription and two facts. The page runs sandboxed with context isolation, so this file
// is CommonJS and nothing of Node reaches the page. The channel names repeat ipc.ts.

import electron = require("electron");

const { contextBridge, ipcRenderer } = electron;

const INVOKE = "c4:invoke";
const EVENT = "c4:event";

interface Payload {
  event: string;
  data: unknown;
}

contextBridge.exposeInMainWorld("c4", {
  invoke: (method: string, params?: unknown): Promise<unknown> =>
    ipcRenderer.invoke(INVOKE, method, params ?? null),
  on: (listener: (payload: Payload) => void): (() => void) => {
    const handler = (_event: Electron.IpcRendererEvent, payload: Payload): void => {
      listener(payload);
    };
    ipcRenderer.on(EVENT, handler);
    return () => {
      ipcRenderer.removeListener(EVENT, handler);
    };
  },
  platform: process.platform,
  versions: {
    electron: process.versions.electron,
    chrome: process.versions.chrome,
    node: process.versions.node,
  },
});
