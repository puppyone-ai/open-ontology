import { useCallback, useEffect, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, Dialog, Notice, PageHeader, Section, Spinner, StatusDot } from "@open-ontology/design-system";
import { ArrowLeft, Database, Play, RefreshCw, ShieldCheck, Trash2 } from "lucide-react";
import { dbKind } from "../../../shared/super-ontology/db-kinds.mjs";
import type { CliError, CliJobState, CliResult, DbSourceInfo } from "../../../shared/super-ontology/bridge";
import { invoke } from "../../model/bridgeInvoke";
import { hasNextStep, runCli, withOperation } from "../../model/cliApi";
import { createOperationId } from "../../model/operationIds";
import { CliCommand, CliErrorNotice } from "../shared/Cli";

type Doctor = { status?: string; sources?: { source_id: string; status: string; error?: string; probe?: { capabilities?: Record<string, { state: string; reason?: string }> }; dba_requirements?: { name: string; dba_statement: string | null; observed?: string; required: string }[]; chunk_plans?: { table: { schema: string; name: string }; estimated_rows: number | null; chunks: number }[] }[]; model?: { status: string } };
type Pipeline = { status?: string; data_published_at?: string | null; last_success?: unknown; last_failure?: unknown; gateway_health?: string; review_state?: string; tables?: { stale?: unknown[]; review_pending?: unknown[] } };

const pad = (n: number) => String(n).padStart(2, "0");
/** Polls one background `oo up|build` run; the page keeps its place while it works. */
function useJob(onDone: (job: CliJobState) => void) {
  const [job, setJob] = useState<CliJobState | null>(null);
  const lines = useRef<string[]>([]);
  const [tick, setTick] = useState(0);
  const doneRef = useRef(onDone);
  doneRef.current = onDone;
  const jobId = useRef<string | null>(null);
  const since = useRef(0);
  const start = useCallback(async (action: "up" | "build"): Promise<{ ok: boolean; error?: CliError }> => {
    const started = await invoke("cli-job-start", { action, operationId: createOperationId("job") });
    if (!started.ok) return { ok: false, error: { code: started.error.code, message: "", hint: null } };
    lines.current = []; since.current = 0; jobId.current = started.value.jobId;
    setJob({ jobId: started.value.jobId, action, state: "running", exit: null, error: null, cli: started.value.cli, startedAt: Date.now(), offset: 0, next: 0, lines: [] });
    setTick((n) => n + 1);
    return { ok: true };
  }, []);
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
  return { job, start };
}

export function DatabaseDetail({ source, onBack, onRemoved }: { source: DbSourceInfo; onBack: () => void; onRemoved: () => void }) {
  const { t } = useLocalization();
  const kind = dbKind(source.backend);
  const config = source.config;
  const [test, setTest] = useState<CliResult<{ reachable: boolean }> | null>(null);
  const [testing, setTesting] = useState(false);
  const [doctor, setDoctor] = useState<CliResult<Doctor> | null>(null);
  const [probing, setProbing] = useState(false);
  const [pipeline, setPipeline] = useState<CliResult<Pipeline> | null>(null);
  const [phase, setPhase] = useState<"idle" | "up" | "build" | "done" | "failed">("idle");
  const [importError, setImportError] = useState<{ error: CliError; cli: string } | null>(null);
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [removing, setRemoving] = useState(false);
  const [removeError, setRemoveError] = useState<{ error: CliError; cli: string } | null>(null);
  const phaseRef = useRef(phase);
  phaseRef.current = phase;

  const readPipeline = useCallback(async () => { setPipeline(await runCli("cli-pipeline", { action: "status" }) as CliResult<Pipeline>); }, []);
  useEffect(() => { void readPipeline(); }, [readPipeline]);

  const job = useJob((finished) => {
    if (finished.state === "failed") { setPhase("failed"); setImportError({ error: finished.error ?? { code: "SO_CLI_FAILED", message: "", hint: null }, cli: finished.cli }); void readPipeline(); return; }
    if (phaseRef.current === "up") { setPhase("build"); void job.start("build"); } else { setPhase("done"); void readPipeline(); }
  });

  const startImport = async () => {
    setImportError(null); setPhase("up");
    const started = await job.start("up");
    if (!started.ok) { setPhase("failed"); setImportError({ error: started.error as CliError, cli: "oo up --config stack.yaml --yes --json" }); }
  };
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
  const pipelineData = pipeline?.ok ? pipeline.data : null;
  const published = pipelineData?.data_published_at ?? null;
  const stale = pipelineData?.tables?.stale?.length ?? 0;
  const pending = pipelineData?.tables?.review_pending?.length ?? 0;
  const freshness = running ? "processing" : published ? (stale > 0 || pending > 0 ? "degraded" : "up_to_date") : "unknown";
  const log = job.job?.lines ?? [];

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
          {config.exclude.length > 0 && <><p className="so-weak">{t("app.db.exclude")}</p><div className="so-pill-row">{config.exclude.map((entry) => <span key={entry} className="so-chip so-mono" data-tone="muted">{entry}</span>)}</div></>}
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
        actions={<><Button variant="secondary" icon={RefreshCw} onClick={() => { void readPipeline(); }} disabled={running} data-testid="database-refresh">{t("app.db.refresh")}</Button><Button variant="primary" icon={Play} loading={running} onClick={() => { void startImport(); }} data-testid="database-import">{running ? t(phase === "up" ? "app.db.import.up" : "app.db.import.build") : t("app.db.import")}</Button></>}>
        <Card className="so-db-pipeline" data-testid="database-pipeline" data-state={freshness}>
          <div className="so-db-pipeline-row">
            <StatusDot tone={freshness === "up_to_date" ? "success" : freshness === "degraded" ? "warning" : freshness === "processing" ? "accent" : "neutral"} pulse={freshness === "processing"} label={t(`app.freshness.${freshness}`)} />
            <span className="so-weak">{published ? t("app.db.pipeline.published", { when: new Date(published).toLocaleString() }) : t("app.db.pipeline.never")}</span>
          </div>
          {pipelineData && <DefinitionList items={[
            { label: t("app.db.pipeline.state"), value: pipelineData.status ?? "unknown" },
            { label: t("app.db.pipeline.stale"), value: String(stale) }, { label: t("app.db.pipeline.review"), value: String(pending) },
          ]} />}
          {pipeline && !pipeline.ok && pipeline.error && <CliErrorNotice error={pipeline.error} cli={pipeline.cli} title={t("app.db.pipeline.failed")} testId="database-pipeline-error" />}
          {running && <div className="so-db-progress" data-testid="database-import-progress" aria-live="polite"><Spinner label={t("app.db.import.working")} /><span>{t(phase === "up" ? "app.db.import.up" : "app.db.import.build")}</span></div>}
          {log.length > 0 && <pre className="so-db-log so-mono" tabIndex={0} role="log" aria-label={t("app.db.import.log")} data-testid="database-import-log">{log.slice(-40).join("\n")}</pre>}
          {phase === "done" && <Notice tone="success" title={t("app.db.import.done")} testId="database-import-done" />}
          {importError && <CliErrorNotice error={importError.error} cli={importError.cli} title={t("app.db.import.failed")} testId="database-import-error" />}
          <CliCommand command={`oo up --config stack.yaml --yes --json\noo build --config stack.yaml --yes --json`} testId="database-import-cli" />
          <p className="so-weak">{t("app.db.import.note", { time: `${pad(0)}` }).replace(/\s*$/u, "")}</p>
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
