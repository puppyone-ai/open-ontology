import { useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, Notice, PageHeader, Section, Segmented, StatusDot, TextArea } from "@open-ontology/design-system";
import { ArrowLeft, ArrowRight, Send } from "lucide-react";
import type { ReviewDigest, ReviewRelation } from "../../../shared/super-ontology/review-digest.mjs";
import type { CliError } from "../../../shared/super-ontology/bridge";
import { CliCommand, CliErrorNotice } from "../shared/Cli";

export type Decision = "adopted" | "rejected";
const ORIGINS = ["declared", "named", "discovered"] as const;
const REASON_LIMIT = 1000;
const percent = (value: number | null) => (value === null ? null : Math.round(value * 1000) / 10);
const pair = (side: { table: string; columns: string[] }) => `${side.table}.${side.columns.join(", ")}`;

/** The sentence that explains one proposed relationship: why the Builder proposed it, or why it holds it back. Unknown rules keep the CLI's own words. */
function useExplanation() {
  const { t } = useLocalization();
  return (relation: ReviewRelation): string => {
    const values = { from: pair(relation.from), to: pair(relation.to), toTable: relation.to.table };
    if (relation.origin === "declared") return t("app.buildReview.why.declared", values);
    if (relation.rule === "relation.no_signal") return t("app.buildReview.why.noSignal", values);
    if (relation.rule === "relation.containment_strict") return t("app.buildReview.why.strict", values);
    if (relation.rule === "relation.containment_with_signal") return t("app.buildReview.why.signal", values);
    return relation.reason || t("app.buildReview.why.other");
  };
}

function Evidence({ relation }: { relation: ReviewRelation }) {
  const { t } = useLocalization();
  const e = relation.evidence;
  const parts: string[] = [];
  if (e.rowsChecked !== null && e.matchedRows !== null) parts.push(t("app.buildReview.evidence.rows", { matched: e.matchedRows, checked: e.rowsChecked }));
  if (e.containment !== null) parts.push(t("app.buildReview.evidence.contained", { percent: percent(e.containment) ?? 0 }));
  parts.push(e.parentUnique ? t("app.buildReview.evidence.unique") : t("app.buildReview.evidence.notUnique"));
  if (e.nameSimilarity !== null) parts.push(t("app.buildReview.evidence.name", { percent: Math.round(e.nameSimilarity * 100) }));
  return <p className="so-review-evidence so-weak" data-testid="review-evidence">{parts.join(" · ")}</p>;
}

function RelationRow({ relation, decision, onChange, explain }: { relation: ReviewRelation; decision: Decision; onChange: (next: Decision) => void; explain: (relation: ReviewRelation) => string }) {
  const { t } = useLocalization();
  const changed = decision !== relation.defaultDecision;
  return (
    <li className="so-review-relation" data-testid="review-relation" data-key={relation.key} data-origin={relation.origin} data-decision={decision} data-changed={changed ? "true" : undefined}>
      <div className="so-review-relation-main">
        <p className="so-review-edge so-mono"><span>{pair(relation.from)}</span><ArrowRight size={14} strokeWidth={1.5} aria-hidden="true" /><span>{pair(relation.to)}</span></p>
        <p className="so-review-why">{explain(relation)}</p>
        <Evidence relation={relation} />
        {relation.fanOut && <p className="so-review-fanout" data-testid="review-fanout">{t("app.buildReview.fanout")}</p>}
      </div>
      <div className="so-review-relation-side">
        <StatusDot tone={decision === "adopted" ? "success" : "neutral"} label={t(decision === "adopted" ? "app.buildReview.state.accepted" : "app.buildReview.state.rejected")} />
        <Segmented<Decision> label={t("app.buildReview.decide", { from: pair(relation.from), to: pair(relation.to) })} value={decision} onChange={onChange} disabled={relation.fanOut && relation.defaultDecision === "rejected"}
          options={[{ value: "adopted", label: t("app.buildReview.accept"), testId: "review-accept" }, { value: "rejected", label: t("app.buildReview.reject"), testId: "review-reject" }]} />
      </div>
    </li>
  );
}

/**
 * The build review (D88): the Builder stopped for the owner. Shows what it proposed (tables, entities, relationships with evidence),
 * lets the person accept or reject each relationship, and hands one decision back to `oo build --resume --review-file`.
 */
export function BuildReview({ name, digest, onBack, onSubmit, submitting, error, cli }: {
  name: string; digest: ReviewDigest; onBack: () => void; onSubmit: (reason: string, decisions: Record<string, Decision>) => void;
  submitting: boolean; error: CliError | null; cli: string;
}) {
  const { t } = useLocalization();
  const explain = useExplanation();
  const [decisions, setDecisions] = useState<Record<string, Decision>>(() => Object.fromEntries(digest.relations.map((relation) => [relation.key, relation.defaultDecision])));
  const [reason, setReason] = useState(() => t("app.buildReview.reason.default"));
  const [touched, setTouched] = useState(false);
  const changes = useMemo(() => digest.relations.filter((relation) => decisions[relation.key] !== relation.defaultDecision), [digest.relations, decisions]);
  const adoptedCount = digest.relations.filter((relation) => decisions[relation.key] === "adopted").length;
  const reasonBad = reason.trim() === "" || reason.length > REASON_LIMIT;
  const submit = () => {
    setTouched(true);
    if (reasonBad) return;
    // Only what differs from the proposal travels: the CLI binds the rest to the summary it already computed.
    onSubmit(reason.trim(), Object.fromEntries(changes.map((relation) => [relation.key, decisions[relation.key] as Decision])));
  };
  const stats: { id: string; value: number; label: string }[] = [
    { id: "tables", value: digest.counts.tables, label: t("app.buildReview.stat.tables") },
    { id: "columns", value: digest.counts.columns, label: t("app.buildReview.stat.columns") },
    { id: "entities", value: digest.counts.entities, label: t("app.buildReview.stat.entities") },
    { id: "relations", value: adoptedCount, label: t("app.buildReview.stat.relations") },
  ];
  const sensitive = digest.counts.sensitiveChecked > 0 ? t("app.buildReview.sensitive", { hidden: digest.counts.sensitiveHidden, checked: digest.counts.sensitiveChecked }) : null;

  return (
    <div className="so-page" data-testid="build-review" data-state={submitting ? "submitting" : error ? "error" : "ready"} data-name={name}>
      <Button variant="ghost" icon={ArrowLeft} onClick={onBack} disabled={submitting} data-testid="review-back">{t("app.buildReview.back", { name })}</Button>
      <PageHeader title={t("app.buildReview.title")} description={t("app.buildReview.description", { name })} />

      <Section title={t("app.buildReview.summary")}>
        <Card className="so-review-stats" data-testid="review-summary" data-hash={digest.summaryHash.slice(0, 12)}>
          {stats.map((stat) => (
            <div key={stat.id} className="so-review-stat" data-testid={`review-stat-${stat.id}`}><span className="so-review-stat-value">{stat.value}</span><span className="so-weak">{stat.label}</span></div>
          ))}
        </Card>
        <p className="so-weak so-review-note">{t("app.buildReview.summaryNote", { declared: digest.counts.declared, discovered: digest.counts.discovered })}{sensitive ? ` ${sensitive}` : ""}</p>
        {digest.discovery.status !== "complete" && <Notice tone="warning" title={t("app.buildReview.partial.title")} testId="review-partial">{t("app.buildReview.partial.body", { count: digest.discovery.unchecked || digest.discovery.remaining })}</Notice>}
      </Section>

      <Section title={t("app.buildReview.entities")} description={t("app.buildReview.entitiesHelp")}>
        <Card className="so-review-entities" as="div" data-testid="review-entities">
          <ul className="so-review-list">
            {digest.entities.map((entity) => (
              <li key={entity.id} className="so-review-entity" data-testid="review-entity" data-table={entity.table}>
                <span className="so-review-entity-name">{entity.label}</span>
                <span className="so-mono so-weak so-truncate">{entity.table}</span>
                <span className="so-weak">{t("app.buildReview.entityKey", { key: entity.key.join(", ") })}</span>
                <span className="so-weak so-review-rows">{entity.rows === null ? "" : t("app.buildReview.entityRows", { count: entity.rows })}</span>
              </li>
            ))}
          </ul>
        </Card>
      </Section>

      <Section title={t("app.buildReview.relations")} description={t("app.buildReview.relationsHelp")}>
        {digest.relations.length === 0 && <Card><p className="so-weak so-review-empty" data-testid="review-no-relations">{t("app.buildReview.noRelations")}</p></Card>}
        {ORIGINS.map((origin) => {
          const rows = digest.relations.filter((relation) => relation.origin === origin);
          if (rows.length === 0) return null;
          return (
            <Card key={origin} as="div" className="so-review-group" data-testid="review-group" data-origin={origin}>
              <div className="so-review-group-head"><h3 className="so-review-group-title">{t(`app.buildReview.origin.${origin}`)}</h3><span className="so-weak">{t(`app.buildReview.origin.${origin}.help`)}</span></div>
              <ul className="so-review-list">
                {rows.map((relation) => <RelationRow key={relation.key} relation={relation} decision={decisions[relation.key] ?? relation.defaultDecision} explain={explain} onChange={(next) => setDecisions((current) => ({ ...current, [relation.key]: next }))} />)}
              </ul>
            </Card>
          );
        })}
      </Section>

      <Section title={t("app.buildReview.confirm")} description={t("app.buildReview.confirmHelp")}>
        <Card className="so-review-confirm" as="div">
          <TextArea label={t("app.buildReview.reason")} value={reason} onChange={(event) => setReason(event.target.value)} rows={2} maxLength={REASON_LIMIT} error={touched && reasonBad ? t("app.db.required") : null} helper={t("app.buildReview.reason.help")} disabled={submitting} data-testid="review-reason" />
          <p className="so-weak" data-testid="review-changes" data-count={changes.length}>{changes.length === 0 ? t("app.buildReview.changes.none") : t("app.buildReview.changes", { count: changes.length })}</p>
          {error && <CliErrorNotice error={error} cli={cli} title={t("app.buildReview.failed")} testId="review-error" />}
          <div className="so-review-actions">
            <Button variant="ghost" onClick={onBack} disabled={submitting} data-testid="review-later">{t("app.buildReview.later")}</Button>
            <Button variant="primary" icon={Send} loading={submitting} onClick={submit} data-testid="review-confirm">{t("app.buildReview.publish")}</Button>
          </div>
          <CliCommand command={`oo build --resume --config stack.yaml --review-file <review.json> --yes --json`} testId="review-cli" />
        </Card>
      </Section>
    </div>
  );
}
