import { useEffect } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Card, EmptyState, Skeleton } from "@open-ontology/design-system";
import { ShieldCheck } from "lucide-react";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";

/** The rules the ontology checks itself against, and how many facts break each one right now. */
export function Conflicts({ onOpenEntity }: { onOpenEntity: (key: string) => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const constraints = useSuperDriveSelector((s) => s.constraints);
  useEffect(() => { void actions.loadConstraints(); }, [actions]);
  void onOpenEntity;
  if (!constraints.loaded) return <Card className="so-list" aria-busy="true">{Array.from({ length: 3 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="50%" height={13} /></div>)}</Card>;
  if (constraints.items.length === 0) return <Card><EmptyState icon={ShieldCheck} title={t("app.conflicts.empty.title")} description={t("app.conflicts.empty.body")} /></Card>;
  return (
    <Card className="so-list" data-testid="constraint-list">
      {constraints.items.map((constraint) => (
        <div key={constraint.id} className="so-list-row">
          <span className="so-list-text"><span className="so-list-title">{constraint.description ?? constraint.id}</span><span className="so-list-sub so-mono">{constraint.id}</span></span>
          {(constraint.violations ?? 0) > 0 ? <Badge tone="danger">{t("app.conflicts.violations", { count: constraint.violations ?? 0 })}</Badge> : <Badge tone="success">{t("app.conflicts.ok")}</Badge>}
        </div>
      ))}
    </Card>
  );
}
