import { describe, expect, it, vi } from "vitest";
import { createAutoHostDecode } from "@so-main/auto-host-decode.mjs";

const drive = (id, backend = "fs") => ({ id, name: id, backend, revision: 1, status: "active", config: { root: `/r/${id}` }, created_at: "2026-01-01T00:00:00Z", updated_at: "2026-01-01T00:00:00Z" });
const file = (id, extra = {}) => ({ id, path: `a/${id}.m4a`, name: `${id}.m4a`, state: "unsupported", state_code: "DOCS_PARSE_CODEC_UNAVAILABLE", b3: `b-${id}`, ...extra });

function fixture({ drives = [drive("d1")], files = { d1: [file("f1")] }, decode = vi.fn(async () => ({ decoded: true })) } = {}) {
  const requests = [];
  const client = {
    async request(name, options = {}) {
      requests.push({ name, ...options });
      if (name === "drivesList") return { value: { items: drives, next_cursor: null, as_of_seq: 1 } };
      if (name === "filesList") return { value: { items: files[options.params.id] ?? [], next_cursor: null, as_of_seq: 1 } };
      if (name === "filesRetry") return { value: { results: [] } };
      throw new Error(`unexpected ${name}`);
    },
  };
  const logger = { warn: vi.fn(), debug: vi.fn() };
  const auto = createAutoHostDecode({ client, decode, logger, createId: () => "id" });
  auto.start();
  return { auto, requests, decode, logger };
}

describe("auto host decode", () => {
  it("decodes a file the engine cannot read itself and asks for a retry", async () => {
    const { auto, requests, decode } = fixture();
    expect(await auto.tick()).toBe(1);
    expect(decode).toHaveBeenCalledWith("f1");
    const retry = requests.find((r) => r.name === "filesRetry");
    expect(retry.body).toEqual({ file_ids: ["f1"] });
    expect(retry.operationId).toBe("host-decode-id");
    expect(requests.find((r) => r.name === "filesList").query).toMatchObject({ state: ["unsupported"], recursive: true });
    auto.stop();
  });

  it("tries each version of a file once, and again when the file changes", async () => {
    const files = { d1: [file("f1")] };
    const { auto, decode } = fixture({ files });
    await auto.tick();
    await auto.tick();
    expect(decode).toHaveBeenCalledTimes(1);
    files.d1 = [file("f1", { b3: "changed" })];
    await auto.tick();
    expect(decode).toHaveBeenCalledTimes(2);
    auto.stop();
  });

  it("leaves other unsupported files and non-fs Drives alone", async () => {
    const files = { d1: [file("k", { state_code: "DOCS_PARSE_UNSUPPORTED_FORMAT" })], d2: [file("s3file")] };
    const { auto, decode } = fixture({ drives: [drive("d1"), drive("d2", "s3")], files });
    expect(await auto.tick()).toBe(0);
    expect(decode).not.toHaveBeenCalled();
    auto.stop();
  });

  it("keeps going after a failed decode and does not repeat it", async () => {
    const decode = vi.fn(async (id) => { if (id === "bad") throw Object.assign(new Error("x"), { code: "SO_MEDIA_DECODE_FAILED" }); return { decoded: true }; });
    const { auto, requests, logger } = fixture({ files: { d1: [file("bad"), file("good")] }, decode });
    expect(await auto.tick()).toBe(1);
    expect(logger.warn).toHaveBeenCalledOnce();
    expect(requests.filter((r) => r.name === "filesRetry")).toHaveLength(1);
    await auto.tick();
    expect(decode).toHaveBeenCalledTimes(2);
    auto.stop();
  });

  it("does nothing once stopped, and a failing engine call only logs", async () => {
    const { auto, decode } = fixture();
    auto.stop();
    expect(await auto.tick()).toBe(0);
    expect(decode).not.toHaveBeenCalled();
    const broken = createAutoHostDecode({ client: { request: async () => { throw new Error("down"); } }, decode, logger: { debug: vi.fn() } });
    broken.start();
    expect(await broken.tick()).toBe(0);
    broken.stop();
  });
});
