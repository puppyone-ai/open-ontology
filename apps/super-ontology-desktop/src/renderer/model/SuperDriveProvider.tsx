import { createContext, useContext, useEffect, useMemo, useRef, useSyncExternalStore, type ReactNode } from "react";
import { superDriveApi } from "./superDriveApi";
import { createSuperDriveStore, type SuperDriveStore } from "./superDriveStore";
import type { SuperDriveState } from "./superDriveState";

const StoreContext = createContext<SuperDriveStore | null>(null);

/** Provides one store to the Super Drive tree and keeps it connected while mounted. A store can be injected for tests. */
export function SuperDriveProvider({ store, manageLifecycle = true, children }: { store?: SuperDriveStore; manageLifecycle?: boolean; children: ReactNode }) {
  const owned = useMemo(() => store ?? createSuperDriveStore({ api: superDriveApi }), [store]);
  useEffect(() => {
    if (!manageLifecycle) return undefined;
    void owned.start();
    return () => owned.stop();
  }, [owned, manageLifecycle]);
  return <StoreContext.Provider value={owned}>{children}</StoreContext.Provider>;
}

export function useSuperDriveStore(): SuperDriveStore {
  const store = useContext(StoreContext);
  if (!store) throw new Error("useSuperDriveStore must be used inside SuperDriveProvider.");
  return store;
}

/** Whole state; its identity changes on every update, so derive with `useSuperDriveSelector` when you can. */
export function useSuperDriveState(): SuperDriveState {
  const store = useSuperDriveStore();
  return useSyncExternalStore(store.subscribe, store.getState, store.getState);
}

/**
 * Selects a slice. The selection is recomputed when the state or the selector
 * changed (an inline selector closes over props such as the current path) and
 * keeps its previous identity while `isEqual` says nothing changed, which is
 * what `useSyncExternalStore` needs to avoid re-render loops.
 */
export function useSuperDriveSelector<T>(selector: (state: SuperDriveState) => T, isEqual: (left: T, right: T) => boolean = Object.is): T {
  const store = useSuperDriveStore();
  const cache = useRef<{ state: SuperDriveState; selector: typeof selector; value: T } | null>(null);
  const read = () => {
    const state = store.getState();
    const cached = cache.current;
    if (cached && cached.state === state && cached.selector === selector) return cached.value;
    const next = selector(state);
    const value = cached && isEqual(cached.value, next) ? cached.value : next;
    cache.current = { state, selector, value };
    return value;
  };
  return useSyncExternalStore(store.subscribe, read, read);
}

export function useSuperDriveActions(): SuperDriveStore["actions"] {
  return useSuperDriveStore().actions;
}
