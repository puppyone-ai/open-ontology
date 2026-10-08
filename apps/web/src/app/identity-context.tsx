import { createContext, useCallback, useContext, useEffect, useState, type ReactNode } from "react";
import { registerSameOriginTransport } from "@open-ontology/sdk-ts";
import { fetchIdentityCurrent, type IdentityView } from "../api/identity-session";

const IdentityContext = createContext<IdentityView>({ status: "absent" });
const RefreshContext = createContext<() => void>(() => undefined);

export function IdentityProvider({ children }: { children: ReactNode }) {
  const [view, setView] = useState<IdentityView>({ status: "pending" });
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    const origin = window.location.origin;
    if (origin.startsWith("https://")) {
      registerSameOriginTransport({ origin, pageOrigin: origin });
    }
  }, []);
  useEffect(() => {
    let cancelled = false;
    void fetchIdentityCurrent().then((next) => {
      if (!cancelled) {
        setView(next);
      }
    });
    return () => {
      cancelled = true;
    };
  }, [attempt]);
  const refresh = useCallback(() => {
    setView({ status: "pending" });
    setAttempt((value) => value + 1);
  }, []);
  return (
    <IdentityContext.Provider value={view}>
      <RefreshContext.Provider value={refresh}>{children}</RefreshContext.Provider>
    </IdentityContext.Provider>
  );
}

export function useIdentityView(): IdentityView {
  return useContext(IdentityContext);
}

export function useRefreshIdentity(): () => void {
  return useContext(RefreshContext);
}
