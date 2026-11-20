import { useCallback, useEffect, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, Dialog, Notice, PageHeader, Section, Spinner, StatusDot } from "@open-ontology/design-system";
import { ArrowLeft, Database, FileText, Network, Play, RefreshCw, ShieldCheck, Table2, Trash2, ClipboardCheck } from "lucide-react";
import { dbKind } from "../../../shared/super-ontology/db-kinds.mjs";
import type { BuildState, CliError, CliJobState, CliResult, DbSourceInfo } from "../../../shared/super-ontology/bridge";
import type { ReviewDigest } from "../../../shared/super-ontology/review-digest.mjs";
import { invoke } from "../../model/bridgeInvoke";
import { hasNextStep, runCli, withOperation } from "../../model/cliApi";
import { createOperationId } from "../../model/operationIds";
import { CliCommand, CliErrorNotice } from "../shared/Cli";
import { useApp } from "../../hooks/useApp";
import { BuildReview, type Decision } from "./BuildReview";

type Doctor = { status?: string; sources?: { source_id: string; status: string; error?: string; probe?: { capabilities?: Record<string, { state: string; reason?: string }> }; dba_requirements?: { name: string; dba_statement: string | null; observed?: string; required: string }[]; chunk_plans?: { table: { schema: string; name: string }; estimated_rows: number | null; chunks: number }[] }[]; model?: { status: string } };
type Pipeline = { status?: string; data_published_at?: string | null; last_success?: unknown; last_failure?: unknown; gateway_health?: string; review_state?: string; tables?: { stale?: unknown[]; review_pending?: unknown[] } };

const STAGE_ORDER = ["load", "candidate", "builder", "self_review", "publish", "gateway", "first_token", "share_prompt"] as const;
const ATLAS_MODE_KEY = "so.atlas.mode";

/** Polls one background `oo up|build` run; the page keeps its place while it works. */
function useJob(onDone: (job: CliJobState) => void) {
  const [job, setJob] = useState<CliJobState | null>(null);
  const lines = useRef<string[]>([]);
  const [tick, setTick] = useState(0);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  const jobId = useRef<string | null>(null);
  const since = useRef(0);
  /** Follows a job the main process already started (a review submitted with its decisions). */
  const track = useCallback((id: string, action: "up" | "build", cli: string) => {
    lines.current = []; since.current = 0; jobId.current = id;
    setJob({ jobId: id, action, state: "running", exit: null, error: null, cli, startedAt: Date.now(), offset: 0, next: 0, lines: [], build: null, gateway: null });
    setTick((n) => n + 1);
  }, []);
  const start = useCallback(async (action: "up" | "build"): Promise<{ ok: boolean; error?: CliError }> => {
    const started = await invoke("cli-job-start", { action, operationId: createOperationId("job") });
    if (!started.ok) return { ok: false, error: { code: started.error.code, message: "", hint: null } };
    track(started.value.jobId, action, started.value.cli);
    return { ok: true };
  }, [track]);
  useEffect(() => {
    if (!jobId.current || job?.state !== "running") return undefined;
    let live = true;
    const timer = window.setInterval(() => {
      void invoke("cli-job-read", { jobId: jobId.current as string, since: since.current }).then((result) => {
        if (!live || !result.ok) return;
        lines.current = [...lines.current, ...result.value.lines].slice(-300);
        since.current = result.value.next;
        const next = { ...result.value, lines: lines.current };
        setJob(next);
        if (result.value.state !== "running") doneRef.current(next);
      });
    }, 800);
    return () => { live = false; window.clearInterval(timer); };
  }, [job?.state, job?.jobId, tick]);
  return { job, start, track };
}

/** The CLI names the cause of a failed stage; these are the few causes with a written next step (the text itself is always shown). */
export function causeHint(message: string): "san" | "grant" | "model" | null {
  if (/subject alternative name|serverAuth/iu.test(message)) return "san";
  if (/pg_read_all_stats|DBA_REQUIREMENT|insufficient privilege|permission denied|grant/iu.test(message)) return "grant";
  if (/\b(401|403|429|5\d\d)\b|unauthori[sz]ed|model/iu.test(message)) return "model";
  return null;
}

type StageTone = "success" | "accent" | "warning" | "danger" | "neutral";
function stageTone(stage: { status: string; error: string | null; name: string } | undefined, review: boolean): StageTone {
  if (!stage) return "neutral";
  if (stage.status === "complete") return "success";
  if (stage.name === "self_review" && review) return "warning";
  if (stage.status === "blocked" || stage.status === "failed" || stage.error) return "danger";
  if (stage.status === "pending") return "neutral";
  return "accent";
}

function BuildStages({ build, services, phase }: { build: BuildState | null; services: "pending" | "running" | "complete"; phase: string }) {
  const { t } = useLocalization();
  const rows = STAGE_ORDER.map((name) => build?.stages.find((stage) => stage.name === name) ?? { name, status: "pending", completed: 0, total: 1, attempts: 0, error: null });
  const running = phase === "build" ? rows.find((stage) => stage.status !== "complete" && stage.status !== "pending") ?? rows.find((stage) => stage.status === "pending") : undefined;
  return (
    <ol className="so-db-stages" data-testid="build-stages" aria-label={t("app.import.stages")}>
      <li data-testid="build-stage" data-stage="services" data-state={services === "complete" ? "complete" : services}>
        <StatusDot tone={services === "complete" ? "success" : services === "running" ? "accent" : "neutral"} pulse={services === "running"} label={t("app.import.stage.services")} />
      </li>
      {rows.map((stage) => {
        const tone = stageTone(stage, build?.review === true);
        const live = stage === running && tone === "accent" ? true : stage.status !== "complete" && stage.status !== "pending" && tone === "accent";
        const count = stage.total > 1 && stage.status !== "complete" && stage.status !== "pending" ? t("app.import.stage.count", { done: stage.completed, total: stage.total }) : null;
        return (
          <li key={stage.name} data-testid="build-stage" data-stage={stage.name} data-state={tone === "warning" ? "review" : stage.status}>
            <StatusDot tone={tone} pulse={live} label={t(`app.import.stage.${stage.name}`)} />
            {count && <span className="so-weak so-mono">{count}</span>}
            {tone === "warning" && <span className="so-weak">{t("app.import.stage.waiting")}</span>}
            {stage.error && tone === "danger" && <span className="so-db-stage-error">{t("app.import.stage.failed")}</span>}
          </li>
        );
      })}
    </ol>
  );
}

export function DatabaseDetail({ source, onBack, onRemoved }: { source: DbSourceInfo; onBack: () => void; onRemoved: () => void }) {
  const { t } = useLocalization();
  const app = useApp();
  const kind = dbKind(source.backend);
  const config = source.config;
  const [test, setTest] = useState<CliResult<{ reachable: boolean }> | null>(null);
  const [testing, setTesting] = useState(false);
  const [doctor, setDoctor] = useState<CliResult<Doctor> | null>(null);
  const [probing, setProbing] = useState(false);
  const [pipeline, setPipeline] = useState<CliResult<Pipeline> | null>(null);
  const [phase, setPhase] = useState<"idle" | "up" | "build" | "review" | "done" | "failed">("idle");
  const [importError, setImportError] = useState<{ error: CliError; cli: string } | null>(null);
  const [build, setBuild] = useState<BuildState | null>(null);
  const [digest, setDigest] = useState<ReviewDigest | null>(null);
  const [reviewing, setReviewing] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<{ error: CliError; cli: string } | null>(null);
  const [adopted, setAdopted] = useState(false);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<{ error: CliError; cli: string } | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  const readPipeline = useCallback(async () => { setPipeline(await runCli("cli-pipeline", { action: "status" }) as CliResult<Pipeline>); }, []);
  /** What the CLI's stage record and review files say now: a build waiting for the owner survives closing the app. */
  const readReview = useCallback(async (): Promise<ReviewDigest | null> => {
    const result = await invoke("cli-review-read", {});
    if (!result.ok) return null;
    setBuild(result.value.build); setDigest(result.value.digest);
    return result.value.digest;
  }, []);
  useEffect(() => { void readPipeline(); }, [readPipeline]);
  useEffect(() => {
    let live = true;
    void invoke("cli-review-read", {}).then((result) => {
      if (!live || !result.ok || phaseRef.current !== "idle") return;
      setBuild(result.value.build); setDigest(result.value.digest);
      if (result.value.build?.review && result.value.digest) setPhase("review");
      else if (result.value.build?.status === "complete") setPhase("done");
    });
    return () => { live = false; };
  }, []);

  const job = useJob((finished) => {
    if (finished.build) setBuild(finished.build);
    if (finished.state === "failed") { setPhase("failed"); setSubmitting(false); setImportError({ error: finished.error ?? { code: "SO_CLI_FAILED", message: "", hint: null }, cli: finished.cli }); void readPipeline(); return; }
    if (finished.state === "review") { setSubmitting(false); void readReview().then((found) => { setPhase("review"); setReviewing(found !== null); }); return; }
    if (finished.action === "up") { setPhase("build"); void job.start("build"); return; }
    setSubmitting(false); setReviewing(false); setAdopted(finished.gateway === true); setPhase("done"); void readPipeline();
  });

  const startImport = async () => {
    setImportError(null); setPhase("up"); setBuild(null);
    const started = await job.start("up");
    if (!started.ok) { setPhase("failed"); setImportError({ error: started.error as CliError, cli: "oo up --config stack.yaml --yes --json" }); }
  };
  const submitReview = async (reason: string, decisions: Record<string, Decision>) => {
    setSubmitting(true); setSubmitError(null);
    const result = await invoke("cli-review-submit", { reason, decisions, operationId: createOperationId("review") });
    const started = result.ok ? result.value : null;
    if (!started || !started.ok) {
      setSubmitting(false);
      setSubmitError(started ? { error: started.error, cli: started.cli } : { error: { code: result.ok ? "SO_CLI_FAILED" : result.error.code, message: "", hint: null }, cli: "" });
      return;
    }
    setReviewing(false); setPhase("build"); setImportError(null);
    job.track(started.jobId, "build", started.cli);
  };
  const openAtlas = () => { window.localStorage.setItem(ATLAS_MODE_KEY, "source"); app.go("ontology", { ontology: "atlas" }); };
  const runTest = async () => { setTesting(true); setTest(await runCli("cli-db-test", { name: source.name })); setTesting(false); };
  const runDoctor = async () => { setProbing(true); setDoctor(await runCli("cli-doctor") as CliResult<Doctor>); setProbing(false); };
  const remove = async () => {
    setRemoving(true);
    const result = await runCli("cli-db-remove", withOperation("db", { name: source.name }));
    setRemoving(false);
    if (result.ok) { setConfirmRemove(false); onRemoved(); } else if (result.error) setRemoveError({ error: result.error, cli: result.cli });
  };

  const mine = doctor?.data?.sources?.find((entry) => entry.source_id === source.name);
  const running = phase === "up" || phase === "build";
  const liveBuild = job.job?.build ?? build;
  const pipelineData = pipeline?.ok ? pipeline.data : null;
  const published = pipelineData?.data_published_at ?? null;
  const stale = pipelineData?.tables?.stale?.length ?? 0;
  const pending = pipelineData?.tables?.review_pending?.length ?? 0;
  const freshness = phase === "review" ? "waiting_review" : running ? "processing" : published ? (stale > 0 || pending > 0 ? "degraded" : "up_to_date") : "unknown";
  const log = job.job?.lines ?? [];
  const hint = importError ? causeHint(`${importError.error.message} ${importError.error.code}`) : null;

  if (reviewing && digest) {
    return <BuildReview name={source.name} digest={digest} onBack={() => { setReviewing(false); setSubmitError(null); }} onSubmit={(reason, decisions) => { void submitReview(reason, decisions); }} submitting={submitting} error={submitError?.error ?? null} cli={submitError?.cli ?? ""} />;
  }

  return (
    <div className="so-page" data-testid="screen-database-detail" data-name={source.name}>
      <Button variant="ghost" icon={ArrowLeft} onClick={onBack} data-testid="database-back">{t("app.sources.back")}</Button>
      <PageHeader title={source.name} description={`${kind?.label ?? source.backend} · ${config.host}:${config.port}`}
        actions={<><Button variant="secondary" icon={Trash2} onClick={() => { setRemoveError(null); setConfirmRemove(true); }} data-testid="database-remove">{t("app.db.remove")}</Button></>} />

      <Section title={t("app.db.section.connection")}>
        <Card className="so-db-card">
          <DefinitionList items={[
            { label: t("app.db.kind"), value: kind?.label ?? source.backend },
            { label: t("app.db.host"), value: <span className="so-mono">{config.host}:{config.port}</span> },
            { label: kind?.nameField === "service" ? t("app.db.service") : t("app.db.database"), value: <span className="so-mono">{config.database}</span> },
            { label: t("app.db.user"), value: <span className="so-mono">{config.user}</span> },
            { label: t("app.db.password"), value: t("app.db.detail.credential") },
            { label: t("app.db.ca"), value: config.tls_ca ? t("app.db.detail.tlsOn") : t("app.db.detail.tlsOff") },
          ]} />
        </Card>
      </Section>

      <Section title={t("app.db.section.tables")} description={t("app.db.section.tablesHelp")}>
        <Card className="so-db-tables" data-testid="database-tables">
          <div className="so-pill-row">{config.include.map((entry) => <span key={entry} className="so-chip so-mono" data-testid="database-include">{entry}</span>)}</div>
          {config.exclude.length > 0 && <><p className="so-weak">{t(`app.db.scope.${kind?.unit ?? "table"}.exclude`)}</p><div className="so-pill-row">{config.exclude.map((entry) => <span key={entry} className="so-chip so-mono" data-tone="muted">{entry}</span>)}</div></>}
        </Card>
      </Section>

      <Section title={t("app.db.section.check")} description={t("app.db.section.checkHelp")}
        actions={<><Button variant="secondary" icon={RefreshCw} loading={testing} onClick={() => { void runTest(); }} data-testid="database-test">{t("app.db.test")}</Button><Button variant="secondary" icon={ShieldCheck} loading={probing} onClick={() => { void runDoctor(); }} data-testid="database-doctor">{t("app.db.doctor")}</Button></>}>
        {test && (test.ok
          ? <Notice tone="success" title={t("app.db.test.ok")} testId="database-test-result"><CliCommand command={test.cli} testId="database-test-cli" /></Notice>
          : <CliErrorNotice error={test.error ?? { code: "SO_CLI_FAILED", message: "", hint: null }} cli={test.cli} title={t("app.db.test.failed")} testId="database-test-error" />)}
        {doctor && !doctor.ok && doctor.error && <CliErrorNotice error={doctor.error} cli={doctor.cli} title={t("app.db.doctor.failed")} testId="database-doctor-error" />}
        {doctor?.ok && (
          <Card className="so-db-doctor" data-testid="database-doctor-result" data-status={mine?.status ?? doctor.data?.status}>
            <div className="so-db-doctor-head"><StatusDot tone={mine?.status === "blocked" ? "danger" : mine?.status === "ok" || mine?.status === "ready" ? "success" : "warning"} label={t("app.db.doctor.status", { status: mine?.status ?? doctor.data?.status ?? "unknown" })} />{mine?.error && <span className="so-mono">{mine.error}</span>}</div>
            {mine?.error && hasNextStep({ code: mine.error.split(/[ :]/u)[0], message: "", hint: null }) && <p className="so-db-next" data-testid="database-doctor-next">{t(`app.cli.next.${mine.error.split(/[ :]/u)[0]}`)}</p>}
            {mine?.probe?.capabilities && (
              <ul className="so-db-caps" aria-label={t("app.db.doctor.capabilities")}>
                {Object.entries(mine.probe.capabilities).map(([name, cap]) => <li key={name} data-state={cap.state}><span className="so-mono">{name}</span><span className="so-weak">{cap.state}{cap.reason ? ` · ${cap.reason}` : ""}</span></li>)}
              </ul>
            )}
            {(mine?.dba_requirements ?? []).length > 0 && (
              <div className="so-db-dba"><p className="so-weak">{t("app.db.doctor.dba")}</p>
                {(mine?.dba_requirements ?? []).filter((r) => r.dba_statement).map((r) => <pre key={r.name} className="so-db-sql so-mono" tabIndex={0}>{r.dba_statement}</pre>)}</div>
            )}
            <CliCommand command={doctor.cli} testId="database-doctor-cli" />
          </Card>
        )}
      </Section>

      <Section title={t("app.db.section.import")} description={t("app.db.section.importHelp")}
        actions={<><Button variant="secondary" icon={RefreshCw} onClick={() => { void readPipeline(); void readReview(); }} disabled={running} data-testid="database-refresh">{t("app.db.refresh")}</Button>
          {phase === "review" && digest
            ? <Button variant="primary" icon={ClipboardCheck} onClick={() => setReviewing(true)} data-testid="database-review-open">{t("app.import.review.open")}</Button>
            : <Button variant="primary" icon={Play} loading={running} onClick={() => { void startImport(); }} data-testid="database-import">{running ? t(phase === "up" ? "app.db.import.up" : "app.db.import.build") : phase === "failed" ? t("app.import.retry") : t("app.db.import")}</Button>}</>}>
        <Card className="so-db-pipeline" data-testid="database-pipeline" data-state={freshness} data-phase={phase}>
          <div className="so-db-pipeline-row">
            <StatusDot tone={freshness === "up_to_date" ? "success" : freshness === "degraded" || freshness === "waiting_review" ? "warning" : freshness === "processing" ? "accent" : "neutral"} pulse={freshness === "processing"} label={freshness === "unknown" ? t("app.db.pipeline.never") : t(`app.freshness.${freshness}`)} />
            {published && <span className="so-weak">{t("app.db.pipeline.published", { when: new Date(published).toLocaleString() })}</span>}
          </div>
          {pipelineData && <DefinitionList items={[
            { label: t("app.db.pipeline.state"), value: pipelineData.status ?? "unknown" },
            { label: t("app.db.pipeline.stale"), value: String(stale) }, { label: t("app.db.pipeline.review"), value: String(pending) },
          ]} />}
          {pipeline && !pipeline.ok && pipeline.error && <CliErrorNotice error={pipeline.error} cli={pipeline.cli} title={t("app.db.pipeline.failed")} testId="database-pipeline-error" />}
          {(phase !== "idle" || liveBuild) && <BuildStages build={liveBuild} services={phase === "up" ? "running" : phase === "idle" && !liveBuild ? "pending" : "complete"} phase={phase} />}
          {running && <div className="so-db-progress" data-testid="database-import-progress" aria-live="polite"><Spinner label={t("app.db.import.working")} /><span>{t(phase === "up" ? "app.db.import.up" : "app.db.import.build")}</span></div>}
          {phase === "review" && <Notice tone="warning" title={t("app.import.review.title")} testId="database-review-waiting"
            action={digest ? <Button variant="secondary" onClick={() => setReviewing(true)} data-testid="database-review-notice-open">{t("app.import.review.open")}</Button> : undefined}>{t("app.import.review.body")}</Notice>}
          {log.length > 0 && <pre className="so-db-log so-mono" tabIndex={0} role="log" aria-label={t("app.db.import.log")} data-testid="database-import-log">{log.slice(-40).join("\n")}</pre>}
          {phase === "done" && (
            <Notice tone="success" title={t("app.import.done.title")} testId="database-import-done">
              <p>{t("app.import.done.body")}</p>
              <div className="so-db-done-actions">
                <Button variant="secondary" icon={Network} onClick={openAtlas} data-testid="database-open-atlas">{t("app.import.done.atlas")}</Button>
                <Button variant="secondary" icon={FileText} onClick={() => app.go("ontology", { ontology: "files" })} data-testid="database-open-files">{t("app.import.done.files")}</Button>
                <Button variant="secondary" icon={Table2} onClick={() => app.go("ontology", { ontology: "query" })} data-testid="database-open-query">{t("app.import.done.query")}</Button>
              </div>
              {!adopted && <p className="so-weak">{t("app.import.done.gateway")}</p>}
            </Notice>
          )}
          {importError && <CliErrorNotice error={importError.error} cli={importError.cli} title={t("app.db.import.failed")} testId="database-import-error" />}
          {importError && hint && <p className="so-db-next" data-testid="database-import-hint">{t(`app.import.hint.${hint}`)}</p>}
          <CliCommand command={`oo up --config stack.yaml --yes --json\noo build --config stack.yaml --yes --json\noo build --resume --config stack.yaml --review-file <review.json> --yes --json`} testId="database-import-cli" />
          <p className="so-weak">{t("app.db.import.note")}</p>
        </Card>
      </Section>

      <div className="so-db-footer so-weak"><Database size={14} strokeWidth={1.5} aria-hidden="true" /><span>{t("super-drive.common.dataLocal")}</span></div>

      {confirmRemove && (
        <Dialog title={t("app.db.remove.title", { name: source.name })} description={t("app.db.remove.body")} onClose={removing ? undefined : () => setConfirmRemove(false)} closeLabel={t("app.common.close")} size="sm"
          footer={<><Button variant="ghost" onClick={() => setConfirmRemove(false)} disabled={removing}>{t("app.common.cancel")}</Button><Button variant="danger" loading={removing} onClick={() => { void remove(); }} data-testid="database-remove-confirm">{t("app.db.remove")}</Button></>}>
          {removeError && <CliErrorNotice error={removeError.error} cli={removeError.cli} title={t("app.db.remove.failed")} testId="database-remove-error" />}
        </Dialog>
      )}
    </div>
  );
}
