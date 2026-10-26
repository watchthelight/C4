// <wtl> project: c4 | file: apps/desktop/electron/protocol.ts | author: watchthelight </wtl>
// The window loads from a scheme of its own, c4://app/, which serves the renderer build and
// nothing else: a path that leaves the directory is refused, every answer carries the content
// security policy, and the page has no other origin to reach.

import { protocol } from "electron";
import { readFile, stat } from "node:fs/promises";
import path from "node:path";

export const SCHEME = "c4";
export const HOST = "app";
/** The page's origin, without a trailing slash. */
export const ORIGIN = `${SCHEME}://${HOST}`;

/** The policy the Tauri window had, with the sources the page never uses closed. */
export const CSP = [
  "default-src 'self'",
  "script-src 'self'",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data:",
  "font-src 'self'",
  "connect-src 'none'",
  "object-src 'none'",
  "base-uri 'none'",
  "form-action 'none'",
  "frame-ancestors 'none'",
].join("; ");

const TYPES: Readonly<Record<string, string>> = {
  ".html": "text/html; charset=utf-8",
  ".js": "text/javascript; charset=utf-8",
  ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8",
  ".json": "application/json; charset=utf-8",
  ".map": "application/json; charset=utf-8",
  ".txt": "text/plain; charset=utf-8",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".ico": "image/x-icon",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
};

/** Before the app is ready: the scheme is treated like http (an origin, relative URLs). */
export function registerScheme(): void {
  protocol.registerSchemesAsPrivileged([
    { scheme: SCHEME, privileges: { standard: true, secure: true, supportFetchAPI: true } },
  ]);
}

function reply(status: number, text: string): Response {
  return new Response(text, {
    status,
    headers: { "content-type": "text/plain; charset=utf-8", "content-security-policy": CSP },
  });
}

/** The file under `root` a request names, or null when the path leaves the directory. */
export function fileFor(root: string, pathname: string): string | null {
  let rel: string;
  try {
    rel = pathname === "/" || pathname === "" ? "/index.html" : decodeURIComponent(pathname);
  } catch {
    return null;
  }
  const file = path.join(root, rel);
  const inside = path.relative(root, file);
  if (inside === "" || inside.startsWith("..") || path.isAbsolute(inside)) {
    return null;
  }
  return file;
}

/** After the app is ready: `c4://app/` is index.html, `c4://app/assets/x.js` that file. */
export function serveRenderer(dir: string): void {
  const root = path.resolve(dir);
  protocol.handle(SCHEME, async (request) => {
    const url = new URL(request.url);
    if (url.host !== HOST || request.method !== "GET") {
      return reply(404, "not found");
    }
    const file = fileFor(root, url.pathname);
    if (file === null) {
      return reply(403, "forbidden");
    }
    let body: Buffer;
    try {
      if (!(await stat(file)).isFile()) {
        return reply(404, "not found");
      }
      body = await readFile(file);
    } catch {
      return reply(404, "not found");
    }
    const type = TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream";
    return new Response(body, {
      status: 200,
      headers: { "content-type": type, "content-security-policy": CSP, "cache-control": "no-cache" },
    });
  });
}
