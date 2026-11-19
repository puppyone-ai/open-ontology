import { CONNECTION_STATES, ENGINE_STATUSES } from "../../shared/super-ontology/engine-contract.mjs";

/**
 * The skeleton never probes or starts anything: it reports a fixed idle
 * snapshot. `preflight` stays null until the environment has been probed.
 */
export function createIdleEngineSnapshot() {
  return {
    revision: 0,
    status: "idle",
    reason: null,
    code: null,
    preflight: null,
    health: null,
    restarts: { count: 0, nextAttemptAt: null },
    ownedByDesktop: false,
    keepEngineOnQuit: false,
    startedAt: null,
    connection: { state: "offline", attempt: 0, nextRetryAt: null, lastSeq: 0 },
  };
}

/**
 * Redacted copy for the renderer. Only known fields leave the main process;
 * credentials, token paths and process details can never be added by accident.
 */
export function toPublicSnapshot(snapshot) {
  if (!ENGINE_STATUSES.includes(snapshot?.status) || !CONNECTION_STATES.includes(snapshot?.connection?.state)) {
    throw new TypeError("Invalid engine snapshot.");
  }
  const { connection, restarts } = snapshot;
  return {
    revision: snapshot.revision,
    status: snapshot.status,
    reason: snapshot.reason ?? null,
    code: snapshot.code ?? null,
    preflight: snapshot.preflight ?? null,
    health: snapshot.health ?? null,
    restarts: { count: restarts?.count ?? 0, nextAttemptAt: restarts?.nextAttemptAt ?? null },
    ownedByDesktop: snapshot.ownedByDesktop === true,
    keepEngineOnQuit: snapshot.keepEngineOnQuit === true,
    startedAt: snapshot.startedAt ?? null,
    connection: {
      state: connection.state,
      attempt: connection.attempt ?? 0,
      nextRetryAt: connection.nextRetryAt ?? null,
      lastSeq: connection.lastSeq ?? 0,
    },
  };
}

/**
 * Holds the one current snapshot. `update` bumps the monotonic `revision` and
 * pushes the redacted copy to the renderer (`engine-changed`); `get` always
 * returns a fresh redacted copy so callers cannot mutate shared state.
 */
export function createSnapshotStore({ broadcast = () => {} } = {}) {
  let current = createIdleEngineSnapshot();
  const listeners = new Set();
  return Object.freeze({
    get: () => toPublicSnapshot(current),
    peek: () => current,
    update(patch) {
      current = { ...current, ...patch, revision: current.revision + 1 };
      const publicSnapshot = toPublicSnapshot(current);
      broadcast("engine-changed", publicSnapshot);
      for (const listener of [...listeners]) {
        try { listener(publicSnapshot); } catch { /* A listener must not break state updates. */ }
      }
      return publicSnapshot;
    },
    /** In-process observers (event bridge); returns the unsubscribe function. */
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  });
}
