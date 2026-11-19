import { useEffect, useRef } from "react";
import { AppProvider, useApp } from "./hooks/useApp";
import { getStore } from "./model/appStore";
import { SuperDriveProvider, useSuperDriveSelector } from "./model/SuperDriveProvider";
import { Nav } from "./shell/Nav";
import { StatusBar } from "./shell/StatusBar";
import { Home } from "./screens/Home";
import { Sources } from "./screens/Sources";
import { Ontology } from "./screens/Ontology";
import { Agents } from "./screens/Agents";
import { SettingsScreen } from "./screens/Settings";
import { FirstRun } from "./screens/FirstRun";

function Screen() {
  const app = useApp();
  switch (app.section) {
    case "sources": return <Sources />;
    case "ontology": return <Ontology />;
    case "agents": return <Agents />;
    case "settings": return <SettingsScreen />;
    default: return <Home />;
  }
}

function Shell() {
  const app = useApp();
  const driveCount = useSuperDriveSelector((state) => state.drives.order.length);
  const drivesLoaded = useSuperDriveSelector((state) => state.drives.loaded);
  // A returning user who already has sources never sees the first run at startup. Asked for later (Settings), it opens normally.
  const decided = useRef(false);
  useEffect(() => {
    if (decided.current || !drivesLoaded) return;
    decided.current = true;
    // Not when the person already started the steps: the drive they just added would otherwise close the first run under them.
    if (app.firstRunOpen && driveCount > 0 && window.sessionStorage.getItem("so.firstRun.engaged") !== "1") app.closeFirstRun(true);
  }, [app, drivesLoaded, driveCount]);
  return (
    <div className="so-app" data-testid="app-root" data-screen={app.firstRunOpen ? "first-run" : app.section}>
      <Nav />
      <main className="so-main" id="so-main" tabIndex={-1}>
        <Screen />
      </main>
      <StatusBar />
      {app.firstRunOpen && <FirstRun />}
    </div>
  );
}

export function App() {
  return (
    <SuperDriveProvider store={getStore()}>
      <AppProvider>
        <Shell />
      </AppProvider>
    </SuperDriveProvider>
  );
}
