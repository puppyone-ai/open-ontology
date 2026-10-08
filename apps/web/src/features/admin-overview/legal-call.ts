import { GROUP_PUBLICATION, createRuntimeRegistry, prepareCall, type PrepareRefusal } from "@open-ontology/sdk-ts";

export function legalAdminCall(operationId: string, current: unknown): PrepareRefusal {
  const registration = createRuntimeRegistry().register({
    group: "B04",
    origin: null,
    ready: false,
    serviceCommit: GROUP_PUBLICATION.B04.contractCommit,
  });
  return prepareCall({
    operationId,
    registration: registration.accepted ? registration : null,
    current,
  });
}
