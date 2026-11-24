import fs from "node:fs/promises";
import path from "node:path";

export const APP_SCHEME = "app";
export const APP_HOST = "renderer";
export const APP_ORIGIN = `${APP_SCHEME}://${APP_HOST}`;
export const APP_SCHEME_PRIVILEGES = Object.freeze({ standard: true, secure: true, supportFetchAPI: true });

/** The renderer never opens a network connection of its own: every call goes through the preload bridge. */
export const CONTENT_SECURITY_POLICY = [
  "default-src 'self'", "base-uri 'none'", "object-src 'none'", "form-action 'none'", "script-src 'self'",
  "style-src 'self' 'unsafe-inline'", "img-src 'self' data: blob: oo-drive:", "font-src 'self' data:",
  "media-src 'self' data: blob: oo-drive:", "connect-src 'self'", "worker-src 'self' blob:", "frame-src 'self' blob: data: oo-drive:",
].join("; ");

const TYPES = Object.freeze({
  ".html": "text/html; charset=utf-8", ".js": "text/javascript; charset=utf-8", ".mjs": "text/javascript; charset=utf-8",
  ".css": "text/css; charset=utf-8", ".json": "application/json", ".svg": "image/svg+xml", ".png": "image/png", ".jpg": "image/jpeg",
  ".webp": "image/webp", ".ico": "image/x-icon", ".woff": "font/woff", ".woff2": "font/woff2", ".ttf": "font/ttf", ".wasm": "application/wasm",
  ".map": "application/json", ".txt": "text/plain; charset=utf-8",
});

const isInside = (root, target) => target === root || target.startsWith(`${root}${path.sep}`);

/** Serves a directory read-only on `app://renderer/…`; a path that leaves the directory (or is a symlink out of it) is a 404. */
export function createAppProtocolHandler({ root, fsPromises = fs }) {
  return async function handle(request) {
    const headers = { "Content-Security-Policy": CONTENT_SECURITY_POLICY, "X-Content-Type-Options": "nosniff", "Cache-Control": "no-store" };
    let url;
    try { url = new URL(request.url); } catch { return new Response(null, { status: 400, headers }); }
    if (url.hostname !== APP_HOST || request.method !== "GET") return new Response(null, { status: 404, headers });
    let relative;
    try { relative = decodeURIComponent(url.pathname); } catch { return new Response(null, { status: 400, headers }); }
    if (relative === "/" || relative === "") relative = "/index.html";
    try {
      // Inside app.asar the real path is the path itself; outside it, symlinks are resolved so a link cannot leave the root.
      const resolve = async (target) => { try { return await fsPromises.realpath(target); } catch { return path.resolve(target); } };
      const realRoot = await resolve(root);
      const real = await resolve(path.join(realRoot, relative));
      if (!isInside(realRoot, real)) return new Response(null, { status: 404, headers });
      const body = await fsPromises.readFile(real);
      return new Response(body, { status: 200, headers: { ...headers, "Content-Type": TYPES[path.extname(real).toLowerCase()] ?? "application/octet-stream" } });
    } catch {
      return new Response(null, { status: 404, headers });
    }
  };
}
