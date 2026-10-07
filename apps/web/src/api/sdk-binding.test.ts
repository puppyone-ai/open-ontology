import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { OPERATIONS, bindingById, invokeOperation } from "@open-ontology/sdk-ts";
import { PACK_METADATA_SCHEMA_SHA256, GATEWAY_ERROR_SCHEMA_SHA256 } from "@open-ontology/sdk-ts";
import { parseGatewayError, parsePackMetadata } from "@open-ontology/sdk-ts";

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(join(process.cwd(), path), "utf8")) as unknown;
}

describe("sdk binding registry", () => {
  it("freezes only the metadata schema and leaves product groups unbound", () => {
    const frozen = OPERATIONS.filter((item) => item.state === "schema_frozen");
    expect(frozen.map((item) => item.operationId)).toEqual(["gateway.packMetadata.get"]);
    expect(frozen[0]?.live).toBe(false);
    expect(frozen[0]?.openApi).toBe("absent");
    expect(bindingById("gateway.sql.post")?.state).toBe("unbound");
    expect(bindingById("B09.query")?.state).toBe("unbound");
    expect(bindingById("B01.oidc")?.method).toBeNull();
    expect(OPERATIONS.every((item) => item.live === false && item.journey === false && item.release === false)).toBe(true);
  });

  it("does not call fetch for a frozen operation", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    expect(invokeOperation("gateway.packMetadata.get")).toEqual({
      ok: false,
      reason: "schema_only",
      operationId: "gateway.packMetadata.get",
    });
    expect(invokeOperation("B08.session")).toEqual({
      ok: false,
      reason: "not_connected",
      operationId: "B08.session",
    });
    expect(invokeOperation("missing")).toEqual({
      ok: false,
      reason: "unknown_operation",
      operationId: "missing",
    });
    expect(fetchSpy).not.toHaveBeenCalled();
    fetchSpy.mockRestore();
  });

  it("parses the shape example and rejects unsafe or incompatible bodies", () => {
    const example = readJson("../../packages/sdk-ts/src/fixtures/pack-metadata.example.json");
    const parsed = parsePackMetadata(example);
    expect(parsed.ok).toBe(true);
    if (parsed.ok) {
      expect(parsed.value.project_id).toBe("depot");
      expect(parsed.value.protocol).toBe("oo.builder.role-pack/v1");
      expect(parsed.value.files[0]?.bytes).toBe(128);
      expect(parsed.unrecognizedKeys).toEqual([]);
    }
    const unsafe = parsePackMetadata({
      ...(example as Record<string, unknown>),
      files: [{ path: "example/pack.json", bytes: Number.MAX_SAFE_INTEGER + 1, sha256: "example-sha" }],
    });
    expect(unsafe).toMatchObject({ ok: false, reason: "unsafe_integer" });
    const oauth = parsePackMetadata({ ...(example as Record<string, unknown>), oauth: true });
    expect(oauth).toMatchObject({ ok: false, reason: "incompatible_enum", path: "$.oauth" });
    const error = parseGatewayError(readJson("../../packages/sdk-ts/src/fixtures/gateway-error.example.json"));
    expect(error.ok).toBe(true);
    expect(parseGatewayError({ error: { code: "OK", message: "x", hint: "y", retryable: false } })).toMatchObject({
      ok: false,
      reason: "incompatible_enum",
    });
  });

  it("matches the published schema file hashes", () => {
    const pack = readFileSync(join(process.cwd(), "../../packages/sdk-ts/src/schema/pack-metadata.schema.json"));
    const gatewayError = readFileSync(join(process.cwd(), "../../packages/sdk-ts/src/schema/gateway-error.schema.json"));
    expect(createHash("sha256").update(pack).digest("hex")).toBe(PACK_METADATA_SCHEMA_SHA256);
    expect(createHash("sha256").update(gatewayError).digest("hex")).toBe(GATEWAY_ERROR_SCHEMA_SHA256);
  });
});
