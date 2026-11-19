import { useCallback, useEffect, useState, type ReactNode } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, Section, Skeleton, StatusDot, type Tone } from "@open-ontology/design-system";
import { Check, Copy, RefreshCw, Stethoscope } from "lucide-react";
import type { CliResult, DiagnosticsReport } from "../../../shared/super-ontology/bridge";
import { invoke } from "../../model/bridgeInvoke";
import { COPY_LIMIT, runCli } from "../../model/cliApi";
import { copyText } from "../../model/format";
import { CliCommand, CliErrorNotice } from "../shared/Cli";

type Doctor = { status?: string; model?: { status?: string }; docs?: { status?: string; why?: string }; sources?: { source_id: string; status: string; error?: string }[] };

function Row({ title, tone, status, children, cli, testId }: { title: string; tone: Tone; status: string; children?: ReactNode; cli?: string; testId: string }) {
  return (
    <div className="so-diag-row" data-testid={testId} data-tone={tone}>
      <div className="so-diag-head"><span className="so-diag-title">{title}</span><StatusDot tone={tone} label={status} /></div>
      {children && <div className="so-diag-body so-weak">{children}</div>}
      {cli && <CliCommand command={cli} testId={`${testId}-cli`} />}
    </div>
  );
}

const toneOf = (result: CliResult | null): Tone => (result === null ? "neutral" : result.ok ? "success" : "warning");

/** Settings > Diagnostics: runtime facts and the CLI's own readiness checks (`oo check`, `oo --version`, `oo docs runtime status`, `oo doctor`). */
export function Diagnostics() {
  const { t } = useLocalization();
  const [report, setReport] = useState<DiagnosticsReport | null>(null);
  const [busy, setBusy] = useState(false);
  const [doctor, setDoctor] = useState<CliResult<Doctor> | null>(null);
  const [probing, setProbing] = useState(false);
  const [copied, setCopied] = useState(false);
  const [large, setLarge] = useState(false);

  const load = useCallback(async () => {
    setBusy(true);
    const result = await invoke("cli-diagnostics", {});
    setBusy(false);
    setReport(result.ok ? result.value : { platform: { os: "", arch: "", node: "", app: null }, paths: { stackFile: null, stackExists: false, secretsDir: "", dataDir: null }, oo: null, version: null, check: null, runtime: null, serve: null });
  }, []);
  useEffect(() => { void load(); }, [load]);
  const runDoctor = async () => { setProbing(true); setDoctor(await runCli("cli-doctor") as CliResult<Doctor>); setProbing(false); };

  const copyReport = async () => {
    if (!report) return;
    const text = JSON.stringify({ ...report, doctor: doctor?.data ?? null }, null, 2);
    if (text.length > COPY_LIMIT) { setLarge(true); return; }
    setLarge(false);
    try { await copyText(text); setCopied(true); window.setTimeout(() => setCopied(false), 1600); } catch { setLarge(true); }
  };

  if (!report) return <Skeleton height={220} />;
  const version = (report.version?.data as { version?: string } | null)?.version ?? null;
  const installed = (report.runtime?.data as { installed?: boolean } | null)?.installed === true;
  const serving = (report.serve?.data as { running?: boolean } | null)?.running === true;
  const checkError = report.check && !report.check.ok ? report.check.error : null;
  return (
    <div className="so-diag" data-testid="settings-diagnostics">
      <Section title={t("app.diag.title")} description={t("app.diag.description")}
        actions={<><Button variant="secondary" icon={RefreshCw} loading={busy} onClick={() => { void load(); }} data-testid="diag-refresh">{t("app.diag.refresh")}</Button><Button variant="secondary" icon={copied ? Check : Copy} onClick={() => { void copyReport(); }} data-testid="diag-copy">{copied ? t("app.common.copied") : t("app.diag.copy")}</Button></>}>
        {large && <p className="so-weak">{t("app.query.copyTooLarge.body")}</p>}
        <Card className="so-diag-list">
          <Row testId="diag-oo" title={t("app.diag.oo")} tone={report.oo ? "success" : "danger"} status={report.oo ? (version ? `v${version}` : t("app.diag.found")) : t("app.diag.missing")} cli={report.version?.cli ?? "oo --version --json"}>
            {report.oo ? <span className="so-mono so-diag-path">{report.oo.path}</span> : t("app.diag.oo.missing")}
          </Row>
          <Row testId="diag-check" title={t("app.diag.check")} tone={toneOf(report.check)} status={report.check ? (report.check.ok ? t("app.diag.ok") : t("app.diag.attention")) : t("app.diag.unknown")} cli={report.check?.cli ?? "oo check --json"}>
            {checkError && <CliErrorNotice error={checkError} cli={report.check?.cli} title={t("app.diag.check.failed")} testId="diag-check-error" />}
            {report.check?.ok && <span>{t("app.diag.check.ok")}</span>}
          </Row>
          <Row testId="diag-runtime" title={t("app.diag.runtime")} tone={report.runtime?.ok ? (installed ? "success" : "neutral") : toneOf(report.runtime)} status={report.runtime ? (report.runtime.ok ? (installed ? t("app.diag.ok") : t("app.diag.missing")) : t("app.diag.attention")) : t("app.diag.unknown")} cli={report.runtime?.cli ?? "oo docs runtime status --json"}>
            {report.runtime?.error ? report.runtime.error.message : report.runtime?.data ? (installed ? t("app.diag.found") : t("app.diag.missing")) : null}
          </Row>
          <Row testId="diag-serve" title={t("app.diag.serve")} tone={report.serve ? (serving ? "success" : "neutral") : "neutral"} status={report.serve ? (serving ? t("app.diag.running") : t("app.diag.stopped")) : t("app.diag.unknown")} cli={report.serve?.cli ?? "oo serve --status --json"}>
            {report.serve?.error?.message}
          </Row>
        </Card>
      </Section>

      <Section title={t("app.diag.environment")}>
        <Card>
          <DefinitionList items={[
            { label: t("app.diag.os"), value: `${report.platform.os} · ${report.platform.arch}` },
            { label: t("app.diag.app"), value: report.platform.app ?? "-" },
            { label: t("app.diag.stack"), value: <span className="so-mono so-diag-path">{report.paths.stackFile ?? "-"}{report.paths.stackExists ? "" : ` (${t("app.diag.stack.none")})`}</span> },
            { label: t("app.diag.secrets"), value: <span className="so-mono so-diag-path">{report.paths.secretsDir}</span> },
          ]} />
        </Card>
      </Section>

      <Section title={t("app.diag.doctor")} description={t("app.diag.doctor.help")} actions={<Button variant="primary" icon={Stethoscope} loading={probing} onClick={() => { void runDoctor(); }} data-testid="diag-doctor">{t("app.diag.doctor.run")}</Button>}>
        {doctor && !doctor.ok && doctor.error && <CliErrorNotice error={doctor.error} cli={doctor.cli} title={t("app.diag.doctor.failed")} testId="diag-doctor-error" />}
        {doctor?.ok && (
          <Card className="so-diag-list" data-testid="diag-doctor-result">
            <Row testId="diag-doctor-model" title={t("app.diag.doctor.model")} tone={doctor.data?.model?.status === "ok" ? "success" : "warning"} status={doctor.data?.model?.status ?? "unknown"} />
            {(doctor.data?.sources ?? []).map((s) => <Row key={s.source_id} testId="diag-doctor-source" title={s.source_id} tone={s.status === "blocked" ? "danger" : s.status === "ok" ? "success" : "warning"} status={s.status}>{s.error && <span className="so-mono">{s.error}</span>}</Row>)}
            {(doctor.data?.sources ?? []).length === 0 && <p className="so-weak so-diag-empty">{t("app.diag.doctor.noSources")}</p>}
            <CliCommand command={doctor.cli} testId="diag-doctor-cli" />
          </Card>
        )}
      </Section>
    </div>
  );
}
