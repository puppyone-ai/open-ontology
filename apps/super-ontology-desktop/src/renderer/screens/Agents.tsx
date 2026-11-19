import { useLocalization } from "@open-ontology/design-system/localization";
import { PageHeader, Tabs } from "@open-ontology/design-system";
import { useApp, type AgentsTab } from "../hooks/useApp";
import { useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDriftedShareCount } from "../model/superDriveSelectors";
import { Connect } from "./agents/Connect";
import { ContextPacks } from "./agents/ContextPacks";
import { AccessLog } from "./agents/AccessLog";

export function Agents() {
  const { t } = useLocalization();
  const app = useApp();
  const drifted = useSuperDriveSelector(selectDriftedShareCount);
  return (
    <div className="so-page" data-testid="screen-agents">
      <PageHeader title={t("app.agents.title")} description={t("app.agents.description")} />
      <div style={{ marginBottom: "var(--so-space-5)" }}>
        <Tabs<AgentsTab> label={t("app.agents.title")} value={app.agentsTab} onChange={app.setAgentsTab}
          tabs={[{ value: "connect", label: t("app.agents.tab.connect") }, { value: "packs", label: t("app.agents.tab.packs"), count: drifted > 0 ? drifted : undefined }, { value: "access", label: t("app.agents.tab.access") }]} />
      </div>
      {app.agentsTab === "connect" && <Connect />}
      {app.agentsTab === "packs" && <ContextPacks />}
      {app.agentsTab === "access" && <AccessLog />}
    </div>
  );
}
