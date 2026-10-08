import { CONTROL_IDENTITY_SERVICE_COMMIT, GROUP_PUBLICATION, SELFBUILD_COMMIT } from "@open-ontology/sdk-ts";

/** Build-time record. No product origin is configured, so every origin stays null. */
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
    ready: GROUP_PUBLICATION.B04.ready,
  },
  sources: {
    ...GROUP_PUBLICATION.B02,
    origin: null,
    reachable: false,
  },
  secrets: {
    ...GROUP_PUBLICATION.B07,
    origin: null,
    reachable: false,
  },
  storybookDeployed: false,
  apiBakedIntoHtml: false,
};
