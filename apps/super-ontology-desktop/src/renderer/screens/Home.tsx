import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, EmptyState, Icon, LinkButton, Notice, PageHeader, Section, Skeleton, StatusDot } from "@open-ontology/design-system";
import { Bot, ChevronRight, Compass, FileText, FolderOpen, FolderPlus, ListChecks, Network, Quote, Shapes, Sparkles, type LucideIcon } from "lucide-react";
import { useApp } from "../hooks/useApp";
import { useLoad } from "../hooks/useLoad";
import { useRelativeTime } from "../hooks/useText";
import { ErrorText } from "./shared/ErrorText";
import { ontologyApi } from "../model/superDriveOntologyApi";
import { useSuperDriveActions, useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDrives, selectOntologyVersion, selectPendingReviewTotal } from "../model/superDriveSelectors";
import { isEngineReady, presentEngine } from "../model/superDriveStateModel";
import { agentApi } from "../model/agentApi";

function Metric({ label, value, count, icon, onClick, loading, testId }: { label: string; value: string; count: number | null; icon: LucideIcon; onClick: () => void; loading?: boolean; testId: string }) {
  return (
    <Card as="div" className="so-metric so-metric-button" role="button" tabIndex={0} onClick={onClick} onKeyDown={(e: React.KeyboardEvent) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); onClick(); } }} data-testid={testId} data-count={count ?? undefined} aria-busy={loading || undefined}>
      <span className="so-metric-head"><Icon icon={icon} size={16} className="so-metric-icon" /><span className="so-metric-label">{label}</span></span>
      <span className="so-metric-value">{loading ? <Skeleton width={48} height={24} /> : value}</span>
    </Card>
  );
}

export function Home() {
  const { t, formatNumber } = useLocalization();
  const relative = useRelativeTime();
  const app = useApp();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const engine = useSuperDriveSelector((s) => s.engine);
  const ready = isEngineReady(engine);
  const pending = useSuperDriveSelector(selectPendingReviewTotal);
  const version = useSuperDriveSelector(selectOntologyVersion);
  const loadedDrives = useSuperDriveSelector((s) => s.drives.loaded);
  const files = drives.reduce((sum, d) => sum + (d.files_total ?? 0), 0);
  const entities = useLoad(() => ontologyApi.searchEntities({ q: "", limit: 200 }), [ready, version], { enabled: ready });
  const versions = useLoad(() => ontologyApi.ontologyVersions({ limit: 6 }), [ready, version], { enabled: ready });
  const tokens = useLoad(() => agentApi.list(), [ready], { enabled: ready });
  const facts = versions.value?.items.reduce((sum, v) => sum + (v.facts_added ?? 0) - (v.facts_retracted ?? 0), 0) ?? 0;
  const entityCount = entities.value ? `${formatNumber(entities.value.items.length)}${entities.value.next_cursor ? "+" : ""}` : "–";
  const presented = presentEngine(engine);

  const suggestions: Array<{ id: string; icon: typeof FolderPlus; title: string; body: string; action: string; run: () => void }> = [];
  if (loadedDrives && drives.length === 0) suggestions.push({ id: "source", icon: FolderPlus, title: t("app.home.next.source.title"), body: t("app.home.next.source.body"), action: t("app.home.next.source.action"), run: () => app.go("sources") });
  if (pending > 0) suggestions.push({ id: "review", icon: ListChecks, title: t("app.home.next.review.title", { count: pending }), body: t("app.home.next.review.body"), action: t("app.home.next.review.action"), run: () => app.go("ontology", { ontology: "review" }) });
  if (drives.length > 0 && tokens.status === "ready" && tokens.value.items.length === 0) suggestions.push({ id: "agent", icon: Bot, title: t("app.home.next.agent.title"), body: t("app.home.next.agent.body"), action: t("app.home.next.agent.action"), run: () => app.go("agents", { agents: "connect" }) });

  if (suggestions.length === 0 && loadedDrives) suggestions.push({ id: "explore", icon: Compass, title: t("app.home.next.explore.title"), body: t("app.home.next.explore.body"), action: t("app.home.next.explore.action"), run: () => app.go("ontology", { ontology: "entities" }) });
  const next = suggestions[0];

  return (
    <div className="so-page" data-testid="screen-home">
      <PageHeader title={t("app.home.title")} description={t("app.home.description")} />

      {!ready && engine !== null && (
        <div style={{ marginBottom: "var(--so-space-5)" }}>
          <Notice tone={presented.tone === "danger" ? "danger" : "warning"} title={t(presented.messageId, presented.values as Record<string, string>)} action={<Button size="compact" onClick={() => app.go("settings", { settings: "engine" })}>{t("app.home.engine.open")}</Button>}>
            {engine?.status === "starting" || engine?.status === "probing" ? t("app.home.engine.wait") : t("app.home.engine.hint")}
          </Notice>
        </div>
      )}

      <div className="so-grid" data-cols="4" style={{ marginBottom: "var(--so-space-6)" }}>
        <Metric icon={FolderOpen} testId="home-count-sources" count={drives.length} label={t("app.home.metric.sources")} value={formatNumber(drives.length)} loading={!loadedDrives && ready} onClick={() => app.go("sources")} />
        <Metric icon={FileText} testId="home-count-files" count={files} label={t("app.home.metric.files")} value={formatNumber(files)} loading={!loadedDrives && ready} onClick={() => app.go("sources")} />
        <Metric icon={Shapes} testId="home-count-entities" count={entities.value ? entities.value.items.length : null} label={t("app.home.metric.entities")} value={entityCount} loading={entities.status === "loading" && !entities.value && ready} onClick={() => app.go("ontology", { ontology: "entities" })} />
        <Metric icon={Quote} testId="home-count-facts" count={versions.value ? Math.max(0, facts) : null} label={t("app.home.metric.facts")} value={versions.value ? formatNumber(Math.max(0, facts)) : "–"} loading={versions.status === "loading" && !versions.value && ready} onClick={() => app.go("ontology", { ontology: "entities" })} />
      </div>

      {next && (
        <Section title={t("app.home.next.title")}>
          <Card className="so-next" data-testid="home-next-step" data-step={next.id}>
            <span className="so-next-icon"><Icon icon={next.icon} size={20} /></span>
            <span className="so-list-text"><span className="so-list-title">{next.title}</span><span className="so-muted">{next.body}</span></span>
            <Button variant={next.id === "source" ? "primary" : "secondary"} onClick={next.run}>{next.action}</Button>
          </Card>
        </Section>
      )}

      {drives.length > 0 && (
        <Section title={t("app.home.sources.title")} actions={<LinkButton icon={ChevronRight} onClick={() => app.go("sources")}>{t("app.home.sources.all")}</LinkButton>}>
          <Card className="so-list">
            {drives.slice(0, 5).map((drive) => {
              const fresh = drive.freshness?.state;
              const label = fresh ? t(`app.freshness.${fresh}`) : t("app.home.sources.unknown");
              return (
                <button key={drive.id} type="button" className="so-list-row" onClick={() => { actions.openDrive(drive.id); app.go("sources"); }}>
                  <span className="so-list-text"><span className="so-list-title so-truncate">{drive.name}</span><span className="so-list-sub">{t("app.home.sources.files", { count: drive.files_total ?? 0 })}</span></span>
                  <StatusDot tone={fresh === "up_to_date" ? "success" : fresh === "degraded" || fresh === "held" ? "warning" : fresh ? "accent" : "neutral"} label={label} pulse={fresh === "processing" || fresh === "syncing"} />
                </button>
              );
            })}
          </Card>
        </Section>
      )}

      <Section title={t("app.home.recent.title")}>
        {versions.status === "error" && !versions.value ? (
          <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={versions.reload}>{t("app.common.retry")}</Button>}><ErrorText error={versions.error} /></Notice>
        ) : versions.value && versions.value.items.length === 0 ? (
          <Card><EmptyState icon={Sparkles} title={t("app.home.recent.empty.title")} description={t("app.home.recent.empty.body")} compact /></Card>
        ) : (
          <Card className="so-list">
            {(versions.value?.items ?? Array.from({ length: 3 }, () => null)).map((version, index) => version ? (
              <div key={version.n} className="so-list-row">
                <span className="so-list-lead"><Network size={16} strokeWidth={1.5} aria-hidden="true" /></span>
                <span className="so-list-text"><span className="so-list-title">{t("app.home.recent.version", { n: version.n })}</span><span className="so-list-sub">{t("app.home.recent.delta", { added: version.facts_added ?? 0, retracted: version.facts_retracted ?? 0 })}</span></span>
                <span className="so-weak">{relative(version.published_at)}</span>
              </div>
            ) : (
              <div key={index} className="so-list-row" aria-hidden="true"><Skeleton width={32} height={32} radius={6} /><span className="so-list-text"><Skeleton width={140} height={13} /><Skeleton width={90} height={11} /></span></div>
            ))}
          </Card>
        )}
      </Section>
    </div>
  );
}
