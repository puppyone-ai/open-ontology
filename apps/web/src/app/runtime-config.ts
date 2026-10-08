import { CONTROL_IDENTITY_COMMIT, SELFBUILD_COMMIT } from "@open-ontology/sdk-ts";

/** Build-time record. Origins stay null until a fixed service is reachable. */
export const runtimeConfig = {
  controlIdentity: {
    state: "schema_frozen" as const,
    origin: null,
    serviceCommit: CONTROL_IDENTITY_COMMIT,
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
