import type { SuperOntologyBridge } from "../../shared/super-ontology/bridge";

declare global {
  interface Window {
    /** Exposed by the preload script (see apps/super-ontology-desktop/PRELOAD.md). Absent in a plain browser. */
    superOntology?: SuperOntologyBridge;
  }
}

export {};
