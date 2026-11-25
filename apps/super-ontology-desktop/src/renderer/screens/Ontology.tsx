import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { PageHeader, Tabs } from "@open-ontology/design-system";
import { useApp, type OntologyTab } from "../hooks/useApp";
import { useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectPendingReviewTotal } from "../model/superDriveSelectors";
import { EntityList } from "./ontology/EntityList";
import { EntityPage } from "./ontology/EntityPage";
import { ReviewQueue } from "./ontology/ReviewQueue";
import { Conflicts } from "./ontology/Conflicts";
import { QueryPanel } from "./ontology/Query";
import { Ask } from "./ontology/Ask";
import { OntologyFiles } from "./ontology/OntologyFiles";
import { Structure } from "./ontology/Structure";
import { Atlas } from "../features/atlas/Atlas";

export function Ontology() {
  const { t } = useLocalization();
  const app = useApp();
  const pending = useSuperDriveSelector(selectPendingReviewTotal);
  const [entityKey, setEntityKey] = useState<string | null>(null);
  useEffect(() => { setEntityKey(null); }, [app.navTick]);

  if ((app.ontologyTab === "entities" || app.ontologyTab === "atlas") && entityKey) return <EntityPage entityKey={entityKey} onBack={() => setEntityKey(null)} onOpen={setEntityKey} />;
  return (
    <div className="so-page" data-testid="screen-ontology" data-wide={app.ontologyTab === "atlas" ? "" : undefined}>
      <PageHeader title={t("app.ontology.title")} description={t("app.ontology.description")} />
      <div style={{ marginBottom: "var(--so-space-5)" }}>
        <Tabs<OntologyTab> label={t("app.ontology.title")} value={app.ontologyTab} onChange={app.setOntologyTab}
          tabs={[{ value: "entities", label: t("app.ontology.tab.entities"), testId: "ontology-tab-entities" }, { value: "atlas", label: t("app.ontology.tab.atlas"), testId: "ontology-tab-atlas" }, { value: "structure", label: t("app.ontology.tab.structure"), testId: "ontology-tab-structure" }, { value: "ask", label: t("app.ontology.tab.ask"), testId: "ontology-tab-ask" }, { value: "query", label: t("app.ontology.tab.query"), testId: "ontology-tab-query" }, { value: "files", label: t("app.ontology.tab.files"), testId: "ontology-tab-files" }, { value: "review", label: t("app.ontology.tab.review"), count: pending > 0 ? pending : undefined, testId: "ontology-tab-review" }, { value: "conflicts", label: t("app.ontology.tab.conflicts"), testId: "ontology-tab-conflicts" }]} />
      </div>
      {app.ontologyTab === "entities" && <EntityList onOpen={setEntityKey} />}
      {app.ontologyTab === "atlas" && <Atlas onOpenEntity={setEntityKey} />}
      {app.ontologyTab === "structure" && <Structure onOpenEntity={(key) => { app.setOntologyTab("entities"); setEntityKey(key); }} />}
      {app.ontologyTab === "ask" && <Ask />}
      {app.ontologyTab === "query" && <QueryPanel />}
      {app.ontologyTab === "files" && <OntologyFiles />}
      {app.ontologyTab === "review" && <ReviewQueue />}
      {app.ontologyTab === "conflicts" && <Conflicts onOpenEntity={(key) => { app.setOntologyTab("entities"); setEntityKey(key); }} />}
    </div>
  );
}
