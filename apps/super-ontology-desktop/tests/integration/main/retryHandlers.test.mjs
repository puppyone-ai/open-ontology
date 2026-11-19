import { describe, expect, it, vi } from "vitest";
import { createControlHandlers, RETRY_BATCH_SIZE } from "@so-main/control-handlers.mjs";
import { parseSuperOntologyRequest } from "@so-shared/requests.mjs";

const DRIVE = "00000000-0000-4000-8000-000000000001";
const OP = "op_0123456789abcdef";
const id = (n) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;

function makeHandlers({ pages }) {
  const calls = [];
  const client = {
    request: vi.fn(async (name, options) => {
      calls.push([name, options]);
      if (name === "filesList") {
        const index = options.query.cursor ? Number(options.query.cursor) : 0;
        const items = pages[index];
        return { value: { items, next_cursor: index + 1 < pages.length ? String(index + 1) : null, as_of_seq: 1 } };
      }
      if (name === "filesRetry") return { value: { results: options.body.file_ids.map((fileId) => ({ id: fileId, status: "ok" })) }, replayed: false };
      throw new Error(`unexpected ${name}`);
    }),
  };
  return { handlers: createControlHandlers({ client, grants: {}, lifecycle: {} }), calls };
}
const file = (n, retryable) => ({ id: id(n), drive_id: DRIVE, path: `f${n}.md`, state: retryable ? "parse_failed" : "published", state_since: "2026-11-17T00:00:00Z", retryable, pending_review: 0 });

describe("files-retry", () => {
  it("sends the ids and the mode with the operation id as the idempotency key", async () => {
    const { handlers, calls } = makeHandlers({ pages: [[]] });
    const result = await handlers["files-retry"]({ operationId: OP, fileIds: [id(1), id(2)], mode: "safe" });
    expect(calls[0]).toEqual(["filesRetry", { body: { file_ids: [id(1), id(2)], mode: "safe" }, operationId: OP }]);
    expect(result.value.results.map((entry) => entry.status)).toEqual(["ok", "ok"]);
  });

  it("validates the request: 1..500 uuids, a known mode, an operation id", () => {
    expect(() => parseSuperOntologyRequest("files-retry", { operationId: OP, fileIds: [] })).toThrow();
    expect(() => parseSuperOntologyRequest("files-retry", { operationId: OP, fileIds: ["nope"] })).toThrow();
    expect(() => parseSuperOntologyRequest("files-retry", { operationId: OP, fileIds: Array.from({ length: 501 }, (_, n) => id(n)) })).toThrow();
    expect(() => parseSuperOntologyRequest("files-retry", { operationId: OP, fileIds: [id(1)], mode: "fast" })).toThrow();
    expect(() => parseSuperOntologyRequest("files-retry", { fileIds: [id(1)] })).toThrow();
    expect(parseSuperOntologyRequest("files-retry", { operationId: OP, fileIds: [id(1)] }).fileIds).toEqual([id(1)]);
  });
});

describe("files-retry-matching", () => {
  it("walks every page of the filter, keeps only retryable files and sends batches of at most 500", async () => {
    const first = Array.from({ length: 500 }, (_, n) => file(n, true));
    const second = Array.from({ length: 500 }, (_, n) => file(1000 + n, n % 2 === 0));
    const third = [file(2000, true)];
    const { handlers, calls } = makeHandlers({ pages: [first, second, third] });
    const result = await handlers["files-retry-matching"]({ operationId: OP, driveId: DRIVE, states: ["parse_failed"], recursive: true });
    expect(result.matched).toBe(751);
    const retries = calls.filter(([name]) => name === "filesRetry");
    expect(retries.map(([, options]) => options.body.file_ids.length)).toEqual([RETRY_BATCH_SIZE, 251]);
    expect(retries.map(([, options]) => options.operationId)).toEqual([`${OP}-0`, `${OP}-1`]);
    expect(result.results).toHaveLength(751);
    expect(calls.find(([name]) => name === "filesList")[1].query).toMatchObject({ state: ["parse_failed"], recursive: true, limit: 500 });
  });

  it("sends nothing when no file in the filter can be retried", async () => {
    const { handlers, calls } = makeHandlers({ pages: [[file(1, false)]] });
    expect(await handlers["files-retry-matching"]({ operationId: OP, driveId: DRIVE })).toEqual({ results: [], matched: 0 });
    expect(calls.some(([name]) => name === "filesRetry")).toBe(false);
  });
});
