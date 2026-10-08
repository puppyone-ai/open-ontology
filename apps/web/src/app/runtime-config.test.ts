import { describe, expect, it } from "vitest";
import { bindingById } from "@open-ontology/sdk-ts";
import { CONTROL_IDENTITY_SERVICE_COMMIT, SELFBUILD_COMMIT } from "@open-ontology/sdk-ts";
import { runtimeConfig } from "./runtime-config";

describe("runtime config", () => {
  it("does not publish a control-identity origin", () => {
    expect(runtimeConfig.controlIdentity.origin).toBeNull();
    expect(runtimeConfig.controlIdentity.state).toBe("fixture_verified");
    expect(runtimeConfig.controlIdentity.reachable).toBe(false);
    expect(runtimeConfig.controlIdentity.serviceCommit).toBe(CONTROL_IDENTITY_SERVICE_COMMIT);
    expect(runtimeConfig.selfbuild.origin).toBeNull();
    expect(runtimeConfig.selfbuild.state).toBe("schema_frozen");
    expect(runtimeConfig.selfbuild.reachable).toBe(false);
    expect(runtimeConfig.selfbuild.serviceCommit).toBe(SELFBUILD_COMMIT);
    expect(runtimeConfig.storybookDeployed).toBe(false);
    expect(runtimeConfig.apiBakedIntoHtml).toBe(false);
    expect(bindingById("getIdentityCurrent")?.state).toBe("fixture_verified");
    expect(bindingById("getIdentityCurrent")?.live).toBe(false);
  });
});
