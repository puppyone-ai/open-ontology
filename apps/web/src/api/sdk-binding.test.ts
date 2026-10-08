import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { OPERATIONS, bindingById, invokeOperation } from "@open-ontology/sdk-ts";
import {
  CONTROL_IDENTITY_COMMIT,
  CONTROL_IDENTITY_SCHEMA_SHA256,
  PACK_METADATA_SCHEMA_SHA256,
  GATEWAY_ERROR_SCHEMA_SHA256,
  SELFBUILD_COMMIT,
  SELFBUILD_HTTP,
  SELFBUILD_SCHEMA_SHA256,
} from "@open-ontology/sdk-ts";
import { parseGatewayError, parsePackMetadata } from "@open-ontology/sdk-ts";

function readJson(path: string): unknown {
  return JSON.parse(readFileSync(join(process.cwd(), path), "utf8")) as unknown;
}

describe("sdk binding registry", () => {
  it("freezes metadata and the control-identity contract, and leaves product groups unbound", () => {
    const frozen = OPERATIONS.filter((item) => item.state === "schema_frozen");
    expect(frozen.map((item) => item.operationId)).toEqual([
      "gateway.packMetadata.get",
      "startAuthorizationCodeLogin",
      "completeAuthorizationCodeCallback",
      "getIdentityCurrent",
      "logoutManagementSession",
      "consumeBootstrapToken",
      ...SELFBUILD_HTTP.map(([operationId]) => operationId),
    ]);
    expect(frozen[0]?.live).toBe(false);
    expect(frozen[0]?.openApi).toBe("absent");
    expect(bindingById("getIdentityCurrent")?.state).toBe("schema_frozen");
    expect(bindingById("getIdentityCurrent")?.live).toBe(false);
    expect(bindingById("getIdentityCurrent")?.serviceCommit).toBe(CONTROL_IDENTITY_COMMIT);
    expect(bindingById("getIdentityCurrent")?.publicFixture).toBeNull();
    expect(bindingById("getIdentityCurrent")?.presentInProcess).toBe(false);
    expect(bindingById("gateway.sql.post")?.state).toBe("unbound");
    expect(bindingById("B09.query")?.state).toBe("unbound");
    expect(bindingById("B01.oidc")?.method).toBeNull();
    expect(bindingById("B01.oidc")?.state).toBe("unbound");
    expect(bindingById("B01.identity")?.state).toBe("unbound");
    expect(bindingById("B01.projectRoleBinding")?.state).toBe("unbound");
    expect(bindingById("B04.builder")?.state).toBe("unbound");
    expect(bindingById("B04.builder")?.method).toBeNull();
    expect(bindingById("createBuild")?.state).toBe("schema_frozen");
    expect(bindingById("createBuild")?.live).toBe(false);
    expect(bindingById("createBuild")?.serviceCommit).toBe(SELFBUILD_COMMIT);
    expect(bindingById("createBuild")?.publicFixture).toBeNull();
    expect(bindingById("createBuild")?.presentInProcess).toBe(false);
    expect(bindingById("decideReviewItem")?.state).toBe("schema_frozen");
    expect(bindingById("decideReviewItem")?.live).toBe(false);
    expect(OPERATIONS.every((item) => item.live === false && item.journey === false && item.release === false)).toBe(true);
  });

  it("does not call fetch for a frozen operation", () => {
    const fetchSpy = vi.spyOn(globalThis, "fetch");
    expect(invokeOperation("gateway.packMetadata.get")).toEqual({
      ok: false,
      reason: "schema_only",
      operationId: "gateway.packMetadata.get",
    });
    expect(invokeOperation("getIdentityCurrent")).toEqual({
      ok: false,
      reason: "schema_only",
      operationId: "getIdentityCurrent",
    });
    expect(invokeOperation("createBuild")).toEqual({
      ok: false,
      reason: "schema_only",
      operationId: "createBuild",
    });
    expect(invokeOperation("decideReviewItem")).toEqual({
      ok: false,
      reason: "schema_only",
      operationId: "decideReviewItem",
    });
    expect(invokeOperation("B04.builder")).toEqual({
      ok: false,
      reason: "not_connected",
      operationId: "B04.builder",
    });
    expect(invokeOperation("B01.identity")).toEqual({
      ok: false,
      reason: "not_connected",
      operationId: "B01.identity",
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
    const identityRoot = join(process.cwd(), "../../tools/control-identity");
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "openapi/control-identity.openapi.yaml"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.openapi,
    );
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "schema/current.schema.json"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.current,
    );
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "schema/logout.schema.json"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.logout,
    );
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "schema/bootstrap-consume.schema.json"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.bootstrapConsume,
    );
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "schema/bootstrap-result.schema.json"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.bootstrapResult,
    );
    expect(createHash("sha256").update(readFileSync(join(identityRoot, "schema/error.schema.json"))).digest("hex")).toBe(
      CONTROL_IDENTITY_SCHEMA_SHA256.error,
    );
    const selfbuildRoot = join(process.cwd(), "../../tools/selfbuild-control-api");
    expect(createHash("sha256").update(readFileSync(join(selfbuildRoot, "openapi/selfbuild-control-api.openapi.yaml"))).digest("hex")).toBe(
      SELFBUILD_SCHEMA_SHA256.openapi,
    );
    expect(createHash("sha256").update(readFileSync(join(selfbuildRoot, "schema/build.schema.json"))).digest("hex")).toBe(
      SELFBUILD_SCHEMA_SHA256.build,
    );
    expect(createHash("sha256").update(readFileSync(join(selfbuildRoot, "schema/create-build.request.schema.json"))).digest("hex")).toBe(
      SELFBUILD_SCHEMA_SHA256.createBuildRequest,
    );
    expect(createHash("sha256").update(readFileSync(join(selfbuildRoot, "schema/error.schema.json"))).digest("hex")).toBe(
      SELFBUILD_SCHEMA_SHA256.error,
    );
  });
});
