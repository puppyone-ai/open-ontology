import { randomBytes } from "node:crypto";
import fs from "node:fs/promises";
import { createReadStream } from "node:fs";
import path from "node:path";
import { Readable } from "node:stream";

/** Read-only access to the original of a file in an fs Drive (20-frontend §3.4.3): `oo-drive://<capability>/<name>`. */
export const DRIVE_FILE_SCHEME = "oo-drive";
/** For `protocol.registerSchemesAsPrivileged` (coordinator patch in main.mjs). No CORS: only embedded viewers load it. */
export const DRIVE_FILE_SCHEME_PRIVILEGES = Object.freeze({ standard: true, secure: true, supportFetchAPI: true, stream: true });

const TOKEN_TTL_MS = 10 * 60_000;
const TOKEN_LIMIT = 200;
const TOKEN_PATTERN = /^[0-9a-f]{64}$/u;

const CONTENT_TYPES = Object.freeze({
  pdf: "application/pdf", png: "image/png", jpg: "image/jpeg", jpeg: "image/jpeg", gif: "image/gif", webp: "image/webp",
  // Never text/html or svg: a Drive file must not run as a page.
  md: "text/plain; charset=utf-8", markdown: "text/plain; charset=utf-8", txt: "text/plain; charset=utf-8", csv: "text/plain; charset=utf-8",
  json: "application/json", html: "text/plain; charset=utf-8", htm: "text/plain; charset=utf-8", svg: "text/plain; charset=utf-8",
});

/** Capability tokens: unguessable, short lived, each bound to one file of one Drive root. */
export function createDriveFileTokens({ now = () => Date.now(), createToken = () => randomBytes(32).toString("hex"), ttlMs = TOKEN_TTL_MS } = {}) {
  const table = new Map();
  return Object.freeze({
    issue({ root, relativePath }) {
      const time = now();
      for (const [token, entry] of table) if (entry.expiresAt <= time) table.delete(token);
      while (table.size >= TOKEN_LIMIT) table.delete(table.keys().next().value);
      const token = createToken();
      table.set(token, { root, relativePath, expiresAt: time + ttlMs });
      return { token, expiresAt: time + ttlMs };
    },
    resolve(token) {
      const entry = typeof token === "string" && TOKEN_PATTERN.test(token) ? table.get(token) : undefined;
      if (!entry || entry.expiresAt <= now()) { table.delete(token); return null; }
      return entry;
    },
    revokeAll() { table.clear(); },
  });
}

const isInside = (parent, child) => {
  const relative = path.relative(parent, child);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
};

/** `Range: bytes=a-b | a- | -n`; null when absent, "invalid" for anything this server does not do (multi ranges). */
export function parseRange(header, size) {
  if (!header) return null;
  const match = /^bytes=(\d*)-(\d*)$/u.exec(header.trim());
  if (!match || (match[1] === "" && match[2] === "")) return "invalid";
  let start;
  let end;
  if (match[1] === "") {
    const suffix = Number(match[2]);
    if (suffix === 0) return "invalid";
    start = Math.max(0, size - suffix);
    end = size - 1;
  } else {
    start = Number(match[1]);
    end = match[2] === "" ? size - 1 : Math.min(Number(match[2]), size - 1);
  }
  if (start >= size || start > end) return "invalid";
  return { start, end };
}

const plain = (status, headers = {}) => new Response(null, { status, headers: { "Cache-Control": "no-store", ...headers } });

/** Serves GET and HEAD with `Range`, `no-store` and `nosniff`; a token only ever reaches its own file. */
export function createDriveFileHandler({ tokens, fsPromises = fs }) {
  return async function handle(request) {
    if (request.method !== "GET" && request.method !== "HEAD") return plain(405, { Allow: "GET, HEAD" });
    let url;
    try { url = new URL(request.url); } catch { return plain(400); }
    const entry = tokens.resolve(url.hostname);
    if (!entry) return plain(404);
    let real;
    let realRoot;
    try {
      realRoot = await fsPromises.realpath(entry.root);
      real = await fsPromises.realpath(path.join(realRoot, ...entry.relativePath.split("/")));
    } catch { return plain(404); }
    if (!isInside(realRoot, real)) return plain(403);
    const stat = await fsPromises.stat(real).catch(() => null);
    if (!stat?.isFile()) return plain(404);
    const extension = path.extname(real).slice(1).toLowerCase();
    const headers = {
      "Content-Type": CONTENT_TYPES[extension] ?? "application/octet-stream", "Accept-Ranges": "bytes", "Cache-Control": "no-store",
      "X-Content-Type-Options": "nosniff", "Content-Security-Policy": "sandbox", "Content-Disposition": "inline",
    };
    const range = parseRange(request.headers.get("range"), stat.size);
    if (range === "invalid") return plain(416, { "Content-Range": `bytes */${stat.size}` });
    const { start, end } = range ?? { start: 0, end: stat.size - 1 };
    const length = stat.size === 0 ? 0 : end - start + 1;
    const status = range ? 206 : 200;
    const responseHeaders = { ...headers, "Content-Length": String(length), ...(range ? { "Content-Range": `bytes ${start}-${end}/${stat.size}` } : {}) };
    if (request.method === "HEAD" || length === 0) return new Response(null, { status, headers: responseHeaders });
    return new Response(Readable.toWeb(createReadStream(real, { start, end })), { status, headers: responseHeaders });
  };
}

/** Registers the handler on Electron's `protocol` (after the scheme was registered as privileged, before windows open). */
export function registerDriveFileProtocol({ protocol, handler }) {
  protocol.handle(DRIVE_FILE_SCHEME, (request) => handler(request));
}

/** `oo-drive://<token>/<file name>`: the name only helps viewers pick a title and a type. */
export function driveFileUrl(token, fileName) {
  return `${DRIVE_FILE_SCHEME}://${token}/${encodeURIComponent(fileName)}`;
}
