import { CONTROL_IDENTITY_SERVICE_COMMIT, SELFBUILD_COMMIT } from "@open-ontology/sdk-ts";

/** Build-time record. The test issuer was destroyed, so the origin stays null. */
export const runtimeConfig = {
  controlIdentity: {
    state: "fixture_verified" as const,
    origin: null,
    serviceCommit: CONTROL_IDENTITY_SERVICE_COMMIT,
    reachable: false,
  },
  selfbuild: {
    state: "schema_frozen" as const,
    origin: null,
    serviceCommit: SELFBUILD_COMMIT,
    reachable: false,
  },
  storybookDeployed: false,
  apiBakedIntoHtml: false,
};
