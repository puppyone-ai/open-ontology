import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, EmptyState, Notice, PageHeader, Skeleton } from "@open-ontology/design-system";
import { ArrowLeft, FileText, ListTree } from "lucide-react";
import { useApp } from "../../hooks/useApp";
import { useErrorText, useRelativeTime } from "../../hooks/useText";
import type { Evidence, Fact } from "../../../shared/super-ontology/docs-api.generated";
import { useSuperDriveActions } from "../../model/SuperDriveProvider";
import type { SuperDriveError } from "../../model/superDriveErrors";
import type { EntityDetail } from "../../model/superDriveOntologyTypes";

function factObject(fact: Fact): string {
  if (fact.object) return fact.object.name;
  const value = fact.value;
  if (!value) return "";
  const first = Object.values(value).find((entry) => typeof entry === "string" || typeof entry === "number");
  return first === undefined ? JSON.stringify(value) : String(first);
}

function EvidenceQuote({ evidence }: { evidence: Evidence }) {
  const { t } = useLocalization();
  const app = useApp();
  return (
    <figure style={{ margin: 0, gap: "var(--so-space-1)" }} className="so-stack">
      {evidence.quote && <blockquote className="so-quote">{evidence.quote}</blockquote>}
      <figcaption className="so-row">
        <FileText size={16} strokeWidth={1.5} className="so-weak so-icon" aria-hidden="true" />
        <Button variant="ghost" size="compact" onClick={() => app.go("sources")} aria-label={t("app.ontology.evidence.open", { path: evidence.path })}><span className="so-mono so-truncate" style={{ maxWidth: 360 }}>{evidence.src.replace(/^\[src:|\]$/g, "")}</span></Button>
      </figcaption>
    </figure>
  );
}

/** One entity: what is known about it as a timeline of facts, every one with the sentence it came from. */
export function EntityPage({ entityKey, onBack, onOpen }: { entityKey: string; onBack: () => void; onOpen: (key: string) => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const relative = useRelativeTime();
  const [entity, setEntity] = useState<EntityDetail | null>(null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const [tick, setTick] = useState(0);

  useEffect(() => {
    let live = true;
    setError(null);
    void actions.loadEntity(entityKey, { includeHistory: false }).then((result) => {
      if (!live) return;
      if (result.ok) setEntity(result.value); else setError(result.error);
    });
    return () => { live = false; };
  }, [actions, entityKey, tick]);

  const facts = (entity?.facts ?? []).filter((fact) => fact.status !== "rejected");
  const ordered = [...facts].sort((a, b) => (a.valid_from ?? a.recorded_at ?? "").localeCompare(b.valid_from ?? b.recorded_at ?? ""));
  const aliases = (entity?.aliases ?? []).map((a) => a.alias).filter((a): a is string => Boolean(a));

  return (
    <div className="so-page" data-testid="screen-entity">
      <div style={{ margin: "calc(-1 * var(--so-space-2)) 0 var(--so-space-2)" }}><Button variant="ghost" size="compact" icon={ArrowLeft} onClick={onBack}>{t("app.ontology.title")}</Button></div>
      {error ? (
        <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={() => setTick((n) => n + 1)}>{t("app.common.retry")}</Button>}>{errorText(error)}</Notice>
      ) : !entity ? (
        <div className="so-stack" aria-busy="true"><Skeleton width={220} height={28} /><Skeleton width={320} height={14} /><Skeleton height={96} radius={8} /></div>
      ) : (
        <>
          <PageHeader title={entity.name} description={[t(`super-drive.entity.type.${entity.type}`), entity.label ?? null].filter(Boolean).join(" · ")} actions={entity.status === "merged" ? <Badge>{t("app.ontology.merged")}</Badge> : undefined} />
          {aliases.length > 0 && <div className="so-pill-row" style={{ marginBottom: "var(--so-space-5)" }} aria-label={t("app.ontology.aliases")}>{aliases.map((alias) => <span key={alias} className="so-chip">{alias}</span>)}</div>}
          <h2 className="so-section-title" style={{ marginBottom: "var(--so-space-3)" }}>{t("app.ontology.timeline")}</h2>
          {ordered.length === 0 ? (
            <Card><EmptyState icon={ListTree} title={t("app.ontology.nofacts.title")} description={t("app.ontology.nofacts.body")} compact /></Card>
          ) : (
            <Card data-testid="fact-list">
              {ordered.map((fact) => (
                <article key={fact.id} className="so-fact">
                  <div className="so-row" data-wrap>
                    {fact.subject.key !== entity.key && <Button variant="ghost" size="compact" onClick={() => onOpen(fact.subject.key)}>{fact.subject.name}</Button>}
                    <span className="so-list-title">{fact.predicate.replace(/_/g, " ")}</span>
                    <span className="so-grow">{fact.object ? <Button variant="ghost" size="compact" onClick={() => onOpen(fact.object!.key)}>{fact.object.name}</Button> : factObject(fact)}</span>
                    {fact.status === "retracted" && <Badge>{t("app.ontology.retracted")}</Badge>}
                    {fact.status === "draft" && <Badge tone="warning">{t("app.ontology.draft")}</Badge>}
                  </div>
                  <span className="so-list-sub">{[fact.valid_from ? t("app.ontology.validFrom", { date: fact.valid_from }) : null, fact.recorded_at ? relative(fact.recorded_at) : null, t("app.ontology.confidence", { value: Math.round(fact.confidence_bp / 100) })].filter(Boolean).join(" · ")}</span>
                  {(fact.evidence ?? []).slice(0, 2).map((evidence) => <EvidenceQuote key={evidence.id} evidence={evidence} />)}
                </article>
              ))}
            </Card>
          )}
        </>
      )}
    </div>
  );
}
