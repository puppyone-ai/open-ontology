import { createContext, useCallback, useContext, useMemo, useState, type ReactNode } from "react";

export type Section = "home" | "sources" | "ontology" | "agents" | "settings";
export type OntologyTab = "entities" | "review" | "conflicts";
export type AgentsTab = "connect" | "packs" | "access";
export type SettingsTab = "general" | "engine" | "model" | "privacy" | "about";

export type Nav = {
  section: Section;
  ontologyTab: OntologyTab;
  agentsTab: AgentsTab;
  settingsTab: SettingsTab;
  navCollapsed: boolean;
};

type Api = Nav & {
  go: (section: Section, tab?: { ontology?: OntologyTab; agents?: AgentsTab; settings?: SettingsTab }) => void;
  setOntologyTab: (tab: OntologyTab) => void;
  setAgentsTab: (tab: AgentsTab) => void;
  setSettingsTab: (tab: SettingsTab) => void;
  toggleNav: () => void;
  firstRunOpen: boolean;
  openFirstRun: () => void;
  closeFirstRun: (completed: boolean) => void;
};

const Context = createContext<Api | null>(null);
const FIRST_RUN_KEY = "so.firstRun.done";

export function AppProvider({ children }: { children: ReactNode }) {
  const [nav, setNav] = useState<Nav>({ section: "home", ontologyTab: "entities", agentsTab: "connect", settingsTab: "general", navCollapsed: window.localStorage.getItem("so.nav.collapsed") === "1" });
  const [firstRunOpen, setFirstRunOpen] = useState(() => window.localStorage.getItem(FIRST_RUN_KEY) !== "1");
  const go = useCallback<Api["go"]>((section, tab) => setNav((c) => ({
    ...c, section, ontologyTab: tab?.ontology ?? c.ontologyTab, agentsTab: tab?.agents ?? c.agentsTab, settingsTab: tab?.settings ?? c.settingsTab,
  })), []);
  const value = useMemo<Api>(() => ({
    ...nav,
    go,
    setOntologyTab: (ontologyTab) => setNav((c) => ({ ...c, ontologyTab })),
    setAgentsTab: (agentsTab) => setNav((c) => ({ ...c, agentsTab })),
    setSettingsTab: (settingsTab) => setNav((c) => ({ ...c, settingsTab })),
    toggleNav: () => setNav((c) => { window.localStorage.setItem("so.nav.collapsed", c.navCollapsed ? "0" : "1"); return { ...c, navCollapsed: !c.navCollapsed }; }),
    firstRunOpen,
    openFirstRun: () => setFirstRunOpen(true),
    closeFirstRun: (completed) => { if (completed) window.localStorage.setItem(FIRST_RUN_KEY, "1"); setFirstRunOpen(false); },
  }), [nav, go, firstRunOpen]);
  return <Context.Provider value={value}>{children}</Context.Provider>;
}

export function useApp(): Api {
  const value = useContext(Context);
  if (!value) throw new Error("useApp must be used inside AppProvider.");
  return value;
}
