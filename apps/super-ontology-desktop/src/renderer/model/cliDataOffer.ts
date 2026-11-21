import type { EngineStateInfo } from "../../shared/super-ontology/bridge";

/**
 * Whether Home offers "Use the CLI's data" (D83: one engine per user). Only while the app runs on its own state, nobody chose yet, and a CLI state
 * with data or a running serve exists. Nothing is moved or deleted by the offer.
 */
export function shouldOfferCliData(state: EngineStateInfo | null | undefined, dismissed: boolean): boolean {
  return !dismissed && state != null && state.mode === "app" && !state.decided && state.cliHasData;
}
