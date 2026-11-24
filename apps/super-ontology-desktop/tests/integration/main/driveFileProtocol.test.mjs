import { chmod, mkdir, mkdtemp, rm, symlink, writeFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  createDriveFileHandler, createDriveFileTokens, driveFileUrl, DRIVE_FILE_SCHEME, parseRange, registerDriveFileProtocol,
} from "@so-main/drive-file-protocol.mjs";
import { createFileHandlers } from "@so-main/file-handlers.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const FID = "00000000-0000-4000-8000-0000000000f1";
const DRIVE = "00000000-0000-4000-8000-000000000001";
let work;
let root;
let tokens;
let handle;
let clock;

beforeEach(async () => {
  work = await mkdtemp(path.join(os.tmpdir(), "so-drive-file-"));
  root = path.join(work, "drive");
  await mkdir(path.join(root, "docs"), { recursive: true });
  await writeFile(path.join(root, "docs", "a.md"), "0123456789");
  await writeFile(path.join(root, "docs", "p.pdf"), "%PDF-fake");
  await writeFile(path.join(root, "docs", "page.html"), "<script>alert(1)</script>");
  await writeFile(path.join(work, "secret.txt"), "outside");
  clock = 1_000;
  tokens = createDriveFileTokens({ now: () => clock });
  handle = createDriveFileHandler({ tokens });
});
afterEach(async () => { await rm(work, { recursive: true, force: true }); });

const get = (token, name = "a.md", init = {}) => handle(new Request(driveFileUrl(token, name), init));
const issue = (relativePath = "docs/a.md") => tokens.issue({ root, relativePath }).token;

describe("drive file protocol", () => {
  it("serves the original with no-store, nosniff and a sandbox", async () => {
    const response = await get(issue());
    expect(response.status).toBe(200);
    expect(await response.text()).toBe("0123456789");
    expect(Object.fromEntries(response.headers)).toMatchObject({
      "cache-control": "no-store", "x-content-type-options": "nosniff", "accept-ranges": "bytes", "content-length": "10",
      "content-type": "text/plain; charset=utf-8", "content-security-policy": "sandbox",
    });
  });

  it("serves a recording with its media type and byte ranges, so the player can start anywhere", async () => {
    for (const [name, type] of [["a.wav", "audio/wav"], ["a.m4a", "audio/mp4"], ["a.mp3", "audio/mpeg"], ["a.mp4", "video/mp4"], ["a.mov", "video/quicktime"], ["a.webm", "video/webm"]]) {
      await writeFile(path.join(root, "docs", name), "RIFF....WAVEdata0123456789");
      const response = await get(issue(`docs/${name}`), name, { headers: { Range: "bytes=12-15" } });
      expect(response.status).toBe(206);
      expect(response.headers.get("content-type")).toBe(type);
      expect(response.headers.get("content-range")).toBe("bytes 12-15/26");
      expect(await response.text()).toBe("data");
    }
  });

  it("answers Range requests: closed, open, suffix, and 416 for the unsatisfiable or multiple ones", async () => {
    const token = issue();
    const closed = await get(token, "a.md", { headers: { Range: "bytes=2-4" } });
    expect(closed.status).toBe(206);
    expect(closed.headers.get("content-range")).toBe("bytes 2-4/10");
    expect(await closed.text()).toBe("234");
    expect(await (await get(token, "a.md", { headers: { Range: "bytes=7-" } })).text()).toBe("789");
    expect(await (await get(token, "a.md", { headers: { Range: "bytes=-3" } })).text()).toBe("789");
    const beyond = await get(token, "a.md", { headers: { Range: "bytes=20-30" } });
    expect(beyond.status).toBe(416);
    expect(beyond.headers.get("content-range")).toBe("bytes */10");
    expect((await get(token, "a.md", { headers: { Range: "bytes=0-1,3-4" } })).status).toBe(416);
    expect(parseRange(null, 10)).toBeNull();
    expect(parseRange("bytes=5-99", 10)).toEqual({ start: 5, end: 9 });
  });

  it("serves HEAD without a body, and refuses other methods", async () => {
    const token = issue();
    const head = await get(token, "a.md", { method: "HEAD" });
    expect([head.status, head.headers.get("content-length"), await head.text()]).toEqual([200, "10", ""]);
    const post = await get(token, "a.md", { method: "POST", body: "x" });
    expect(post.status).toBe(405);
  });

  it("never serves a Drive file as a page", async () => {
    const response = await get(issue("docs/page.html"), "page.html");
    expect(response.headers.get("content-type")).toBe("text/plain; charset=utf-8");
    expect((await get(issue("docs/p.pdf"), "p.pdf")).headers.get("content-type")).toBe("application/pdf");
  });

  it("rejects unknown, malformed and expired capabilities", async () => {
    expect((await get("f".repeat(64))).status).toBe(404);
    expect((await get("not-a-token")).status).toBe(404);
    const token = issue();
    clock += 11 * 60_000;
    expect((await get(token)).status).toBe(404);
  });

  it("does not leave the Drive root, not by path and not by symlink", async () => {
    expect((await get(issue("../secret.txt"), "secret.txt")).status).toBe(403);
    await symlink(path.join(work, "secret.txt"), path.join(root, "docs", "link.txt"));
    expect((await get(issue("docs/link.txt"), "link.txt")).status).toBe(403);
    expect((await get(issue("docs/missing.md"))).status).toBe(404);
  });

  it("caps the table, dropping the oldest capability first, and can revoke all", () => {
    const first = tokens.issue({ root, relativePath: "a" }).token;
    for (let index = 0; index < 250; index += 1) tokens.issue({ root, relativePath: `f${index}` });
    expect(tokens.resolve(first)).toBeNull();
    tokens.revokeAll();
    expect(tokens.resolve(tokens.issue({ root, relativePath: "z" }).token)).not.toBeNull();
  });

  it("registers on the protocol under its scheme", () => {
    const protocol = { handle: vi.fn() };
    registerDriveFileProtocol({ protocol, handler: handle });
    expect(protocol.handle).toHaveBeenCalledWith(DRIVE_FILE_SCHEME, expect.any(Function));
  });
});

describe("file handlers", () => {
  function make({ backend = "fs", path: filePath = "docs/a.md" } = {}) {
    const client = {
      request: vi.fn(async (name) => {
        if (name === "fileDetail") return { value: { id: FID, drive_id: DRIVE, path: filePath, state: "published" } };
        if (name === "driveGet") return { value: { id: DRIVE, name: "d", backend, status: "active", revision: 1, config: { root } } };
        if (name === "fileBlocks") return { value: { items: [{ id: "blk_1" }], next_cursor: null, as_of_seq: 1 } };
        throw new Error(name);
      }),
    };
    return { client, handlers: createFileHandlers({ client, tokens }) };
  }

  it("issues a preview URL only for fs Drives and the file's own path", async () => {
    const { handlers } = make();
    const { url, expiresAt } = await handlers["file-preview-url"]({ fileId: FID });
    expect(url).toMatch(/^oo-drive:\/\/[0-9a-f]{64}\/a\.md$/u);
    expect(Number.isNaN(Date.parse(expiresAt))).toBe(false);
    const response = await handle(new Request(url));
    expect(await response.text()).toBe("0123456789");
    await expect(make({ backend: "s3" }).handlers["file-preview-url"]({ fileId: FID })).rejects.toMatchObject({ code: "SO_PREVIEW_UNAVAILABLE" });
  });

  it("passes the includes and the paging to the engine", async () => {
    const { handlers, client } = make();
    await handlers["file-detail"]({ fileId: FID, include: ["history"] });
    expect(client.request).toHaveBeenCalledWith("fileDetail", { params: { fid: FID }, query: { include: ["history"] } });
    expect((await handlers["file-blocks"]({ fileId: FID, page: 2, limit: 100 })).items).toHaveLength(1);
    expect(client.request).toHaveBeenLastCalledWith("fileBlocks", { params: { fid: FID }, query: { page: 2, limit: 100, cursor: undefined } });
  });

  it("validates the three requests", () => {
    expect(parseSuperOntologyRequest("file-detail", { fileId: FID, include: ["history", "entities"] }).include).toEqual(["history", "entities"]);
    for (const bad of [
      ["file-detail", { fileId: "x" }], ["file-detail", { fileId: FID, include: ["everything"] }], ["file-blocks", { fileId: FID, limit: 501 }],
      ["file-blocks", { fileId: FID, page: 0 }], ["file-preview-url", { fileId: FID, path: "/etc/passwd" }],
    ]) expect(() => parseSuperOntologyRequest(bad[0], bad[1]), JSON.stringify(bad)).toThrow();
    void chmod;
  });
});
