import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, EmptyState, Icon, Notice, PageHeader, Skeleton } from "@open-ontology/design-system";
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

/** A source the fact came from: the file name, then where in it. One click opens the file in its source. */
function SourceChip({ evidence }: { evidence: Evidence }) {
  const { t } = useLocalization();
  const app = useApp();
  const name = evidence.path.slice(evidence.path.lastIndexOf("/") + 1);
  const anchor = evidence.src.includes("#") ? `#${evidence.src.slice(evidence.src.indexOf("#") + 1).replace(/\]$/u, "")}` : "";
  return (
    <button type="button" className="so-source-chip" data-testid="evidence-link" data-file={evidence.path} onClick={() => app.openFile(evidence.file_id)} aria-label={t("app.ontology.evidence.open", { path: evidence.path })}>
      <Icon icon={FileText} size={16} className="so-source-chip-icon" />
      <span className="so-source-chip-name so-truncate">{name}</span>
      {anchor && <span className="so-source-chip-anchor so-mono so-truncate">{anchor}</span>}
    </button>
  );
}

function EntityLink({ entity, onOpen }: { entity: { key: string; name: string }; onOpen: (key: string) => void }) {
  return <button type="button" className="so-entity-link" onClick={() => onOpen(entity.key)}>{entity.name}</button>;
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
  const sourceCount = new Set(ordered.flatMap((fact) => (fact.evidence ?? []).map((evidence) => evidence.file_id))).size;
  const aliases = (entity?.aliases ?? []).map((a) => a.alias).filter((a): a is string => Boolean(a));

  return (
    <div className="so-page" data-testid="screen-entity" data-name={entity?.name}>
      <div style={{ margin: "calc(-1 * var(--so-space-2)) 0 var(--so-space-2)" }}><Button variant="ghost" size="compact" icon={ArrowLeft} onClick={onBack}>{t("app.ontology.title")}</Button></div>
      {error ? (
        <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={() => setTick((n) => n + 1)}>{t("app.common.retry")}</Button>}>{errorText(error)}</Notice>
      ) : !entity ? (
        <div className="so-stack" aria-busy="true"><Skeleton width={220} height={28} /><Skeleton width={320} height={14} /><Skeleton height={96} radius={8} /></div>
      ) : (
        <>
          <PageHeader title={entity.name} titleAdornment={<Badge>{t(`super-drive.entity.type.${entity.type}`)}</Badge>}
            description={[entity.label && entity.label !== entity.name ? entity.label : null, t("app.ontology.summary", { facts: ordered.length, sources: sourceCount, aliases: aliases.length })].filter(Boolean).join(" · ")}
            actions={entity.status === "merged" ? <Badge>{t("app.ontology.merged")}</Badge> : undefined} />
          {aliases.length > 0 && <div className="so-row" style={{ marginBottom: "var(--so-space-5)" }} data-wrap><span className="so-weak">{t("app.ontology.aliases")}</span><span className="so-pill-row" role="list">{aliases.map((alias) => <span key={alias} role="listitem" className="so-chip">{alias}</span>)}</span></div>}
          <h2 className="so-section-title" style={{ marginBottom: "var(--so-space-3)" }}>{t("app.ontology.timeline")}</h2>
          {ordered.length === 0 ? (
            <Card><EmptyState icon={ListTree} title={t("app.ontology.nofacts.title")} description={t("app.ontology.nofacts.body")} compact /></Card>
          ) : (
            <Card className="so-facts" data-testid="fact-list">
              {ordered.map((fact) => (
                <article key={fact.id} className="so-fact" data-testid="fact-row" data-predicate={fact.predicate} data-state={fact.status}>
                  <p className="so-fact-line">
                    {fact.subject.key === entity.key ? <span className="so-fact-self">{fact.subject.name}</span> : <EntityLink entity={fact.subject} onOpen={onOpen} />}
                    <span className="so-fact-predicate">{fact.predicate.replace(/_/g, " ")}</span>
                    {fact.object ? <EntityLink entity={fact.object} onOpen={onOpen} /> : <span>{factObject(fact)}</span>}
                    {fact.status === "retracted" && <Badge>{t("app.ontology.retracted")}</Badge>}
                    {fact.status === "draft" && <Badge tone="warning">{t("app.ontology.draft")}</Badge>}
                  </p>
                  <p className="so-fact-meta">{[fact.valid_from ? t("app.ontology.validFrom", { date: fact.valid_from }) : null, fact.recorded_at ? relative(fact.recorded_at) : null, t("app.ontology.confidence", { value: Math.round(fact.confidence_bp / 100) })].filter(Boolean).join(" · ")}</p>
                  {(fact.evidence ?? []).slice(0, 2).map((evidence) => evidence.quote ? <blockquote key={`q-${evidence.id}`} className="so-quote">{evidence.quote}</blockquote> : null)}
                  {(fact.evidence ?? []).length > 0 && <div className="so-source-row">{(fact.evidence ?? []).slice(0, 3).map((evidence) => <SourceChip key={evidence.id} evidence={evidence} />)}</div>}
                </article>
              ))}
            </Card>
          )}
        </>
      )}
    </div>
  );
}
