import { useCallback, useEffect, useRef, useState } from "react";
import { toSuperDriveError, type SuperDriveError } from "../model/superDriveErrors";
import type { ApiResult } from "../model/bridgeInvoke";

export type Loaded<T> =
  | { status: "loading"; value: T | null; error: null }
  | { status: "ready"; value: T; error: null }
  | { status: "error"; value: T | null; error: SuperDriveError };

/** Runs an API call when the inputs change; keeps the last value while reloading so the screen does not flash empty. */
export function useLoad<T>(run: () => Promise<ApiResult<T>>, deps: ReadonlyArray<unknown>, { enabled = true }: { enabled?: boolean } = {}): Loaded<T> & { reload: () => void } {
  const [state, setState] = useState<Loaded<T>>({ status: "loading", value: null, error: null });
  const token = useRef(0);
  const [tick, setTick] = useState(0);
  const runRef = useRef(run);
  runRef.current = run;
  useEffect(() => {
    if (!enabled) return undefined;
    const mine = ++token.current;
    setState((current) => ({ status: "loading", value: current.value, error: null }));
    void runRef.current().then((result) => {
      if (mine !== token.current) return;
      if (result.ok) setState({ status: "ready", value: result.value, error: null });
      else setState((current) => ({ status: "error", value: current.value, error: toSuperDriveError(result) }));
    });
    return () => { token.current += 1; };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [...deps, enabled, tick]);
  const reload = useCallback(() => setTick((n) => n + 1), []);
  return { ...state, reload } as Loaded<T> & { reload: () => void };
}
