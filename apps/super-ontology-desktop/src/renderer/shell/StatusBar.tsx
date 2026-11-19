import { useLocalization } from "@open-ontology/design-system/localization";
import { StatusDot, Tooltip, type Tone } from "@open-ontology/design-system";
import { useApp } from "../hooks/useApp";
import { useSuperDriveSelector } from "../model/SuperDriveProvider";
import { presentEngine, type Tone as ModelTone } from "../model/superDriveStateModel";

const TONE: Record<ModelTone, Tone> = { neutral: "neutral", info: "accent", success: "success", warning: "warning", danger: "danger" };

/** Engine state and the number of jobs in flight; a click opens the engine details. */
export function StatusBar() {
  const { t } = useLocalization();
  const app = useApp();
  const engine = useSuperDriveSelector((state) => state.engine);
  const processing = useSuperDriveSelector((state) => state.drives.order.reduce((sum, id) => {
    const counts = state.drives.byId[id]?.counts ?? {};
    return sum + ["queued", "parsing", "extracting"].reduce((n, key) => n + (typeof counts[key] === "number" ? (counts[key] as number) : 0), 0);
  }, 0));
  const presented = presentEngine(engine);
  const label = t(presented.pillId);
  const engineState = presented.status === "healthy" || presented.status === "degraded" ? "running" : ["starting", "probing", "restarting"].includes(presented.status) ? "starting" : "unavailable";
  return (
    <footer className="so-statusbar">
      <Tooltip label={t("app.status.openEngine")}>
        <button type="button" className="so-statusbar-button" onClick={() => app.go("settings", { settings: "engine" })} data-testid="statusbar-engine" data-state={engineState}>
          <StatusDot tone={TONE[presented.tone]} label={t("app.status.engine", { state: label })} pulse={presented.busy} />
        </button>
      </Tooltip>
      {processing > 0 ? (
        <button type="button" className="so-statusbar-button" onClick={() => app.go("sources")} data-testid="statusbar-jobs" data-count={processing}>
          <StatusDot tone="accent" label={t("app.status.jobs", { count: processing })} pulse />
        </button>
      ) : <span hidden data-testid="statusbar-jobs" data-count="0" />}
    </footer>
  );
}
