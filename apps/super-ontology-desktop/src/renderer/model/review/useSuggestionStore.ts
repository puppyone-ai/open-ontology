import { useMemo, useSyncExternalStore } from "react";
import { useSuperDriveStore } from "../SuperDriveProvider";
import { createSuggestionStore, type SuggestionState, type SuggestionStore } from "./suggestionStore";

/** One suggestion store per API (so per Super Drive store), with its state as a React value. */
export function useSuggestionStore(): { store: SuggestionStore; state: SuggestionState } {
  const { api } = useSuperDriveStore();
  const store = useMemo(() => createSuggestionStore({ api }), [api]);
  const state = useSyncExternalStore(store.subscribe, store.getState, store.getState);
  return { store, state };
}
