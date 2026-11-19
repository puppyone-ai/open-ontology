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

export function Ontology() {
  const { t } = useLocalization();
  const app = useApp();
  const pending = useSuperDriveSelector(selectPendingReviewTotal);
  const [entityKey, setEntityKey] = useState<string | null>(null);
  useEffect(() => { setEntityKey(null); }, [app.navTick]);

  if (app.ontologyTab === "entities" && entityKey) return <EntityPage entityKey={entityKey} onBack={() => setEntityKey(null)} onOpen={setEntityKey} />;
  return (
    <div className="so-page" data-testid="screen-ontology">
      <PageHeader title={t("app.ontology.title")} description={t("app.ontology.description")} />
      <div style={{ marginBottom: "var(--so-space-5)" }}>
        <Tabs<OntologyTab> label={t("app.ontology.title")} value={app.ontologyTab} onChange={app.setOntologyTab}
          tabs={[{ value: "entities", label: t("app.ontology.tab.entities"), testId: "ontology-tab-entities" }, { value: "review", label: t("app.ontology.tab.review"), count: pending > 0 ? pending : undefined, testId: "ontology-tab-review" }, { value: "conflicts", label: t("app.ontology.tab.conflicts"), testId: "ontology-tab-conflicts" }]} />
      </div>
      {app.ontologyTab === "entities" && <EntityList onOpen={setEntityKey} />}
      {app.ontologyTab === "review" && <ReviewQueue />}
      {app.ontologyTab === "conflicts" && <Conflicts onOpenEntity={(key) => { app.setOntologyTab("entities"); setEntityKey(key); }} />}
    </div>
  );
}
