import { Fragment, useCallback, useEffect, useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, EmptyState, Notice, Select, Skeleton } from "@open-ontology/design-system";
import { Check, Copy, Download, FileText, GitCompare } from "lucide-react";
import type { OntologyVersion } from "../../../shared/super-ontology/docs-api.generated";
import { invoke } from "../../model/bridgeInvoke";
import { COPY_LIMIT, runCli } from "../../model/cliApi";
import { copyText } from "../../model/format";
import { createOperationId } from "../../model/operationIds";
import { diffLines, namesFrom, tokenizeYamlLine, toYaml, type DiffLine } from "../../model/cliFormat";
import { CliCommand, CliErrorNotice } from "../shared/Cli";

type OntologyFile = { id: string; name: string; group: "version" | "gateway"; text: string };

/** The YAML manifest of one ontology version (what the engine records about it). */
export function versionYaml(version: OntologyVersion): string {
  const { n, as_of, published_at, facts_added, facts_retracted, gates, covers_drive_rev, pinned_by } = version;
  return `# Ontology version ${n}\n${toYaml({ version: n, as_of, published_at, facts_added, facts_retracted, covers_drive_rev, pinned_by, gates })}`;
}

export function YamlView({ text, name }: { text: string; name: string }) {
  const lines = useMemo(() => text.replace(/\n$/u, "").split("\n"), [text]);
  return (
    <div className="so-yaml" role="region" aria-label={name} tabIndex={0} data-testid="ontology-yaml">
      <ol className="so-yaml-lines">{lines.map((line, index) => (
        <li key={index} className="so-yaml-line"><span className="so-yaml-no" aria-hidden="true">{index + 1}</span><code className="so-yaml-text">{tokenizeYamlLine(line).map((token, i) => <span key={i} data-token={token.kind}>{token.text}</span>)}{line === "" ? "​" : null}</code></li>
      ))}</ol>
    </div>
  );
}

export function DiffView({ lines, name }: { lines: DiffLine[]; name: string }) {
  const changed = lines.filter((l) => l.kind !== "same").length;
  return (
    <div className="so-yaml so-diff" role="region" aria-label={name} tabIndex={0} data-testid="ontology-diff" data-changes={changed}>
      <ol className="so-yaml-lines">{lines.map((line, index) => (
        <li key={index} className="so-yaml-line" data-diff={line.kind}><span className="so-yaml-no" aria-hidden="true">{line.left ?? ""}</span><span className="so-yaml-no" aria-hidden="true">{line.right ?? ""}</span><span className="so-yaml-sign" aria-hidden="true">{line.kind === "add" ? "+" : line.kind === "del" ? "-" : " "}</span><code className="so-yaml-text">{line.text || "​"}{line.kind !== "same" && <span className="so-visually-hidden">{line.kind === "add" ? " (added)" : " (removed)"}</span>}</code></li>
      ))}</ol>
    </div>
  );
}

/** Ontology > Files: the ontology as YAML, per version, read-only, with a version diff and export. */
export function OntologyFiles() {
  const { t } = useLocalization();
  const [versions, setVersions] = useState<OntologyVersion[] | null>(null);
  const [failed, setFailed] = useState(false);
  const [gatewayFiles, setGatewayFiles] = useState<OntologyFile[]>([]);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [compareId, setCompareId] = useState<string>("");
  const [copied, setCopied] = useState(false);
  const [tooLarge, setTooLarge] = useState(false);
  const [exported, setExported] = useState<{ directory: string; count: number; cli: string } | null>(null);
  const [exportFailed, setExportFailed] = useState(false);
  const [prefix, setPrefix] = useState("");
  const [lookupError, setLookupError] = useState<{ error: { code: string; message: string; hint: string | null }; cli: string } | null>(null);

  useEffect(() => {
    let live = true;
    void invoke("ontology-versions", { limit: 100 }).then((result) => {
      if (!live) return;
      if (result.ok) setVersions((result.value.items as unknown as OntologyVersion[]).slice().sort((a, b) => b.n - a.n)); else { setFailed(true); setVersions([]); }
    });
    return () => { live = false; };
  }, []);

  const versionFiles = useMemo<OntologyFile[]>(() => (versions ?? []).map((v) => ({ id: `v${v.n}`, name: `ontology-v${v.n}.yaml`, group: "version", text: versionYaml(v) })), [versions]);
  const files = useMemo(() => [...versionFiles, ...gatewayFiles], [versionFiles, gatewayFiles]);
  useEffect(() => { if (!selectedId && files.length > 0) setSelectedId(files[0].id); }, [files, selectedId]);
  const selected = files.find((f) => f.id === selectedId) ?? null;
  const compare = files.find((f) => f.id === compareId) ?? null;
  const diff = useMemo(() => (selected && compare ? diffLines(compare.text, selected.text) : null), [selected, compare]);

  const loadGateway = useCallback(async () => {
    setLookupError(null);
    const found = await runCli("cli-query", { op: "knowledge-search", text: ".yaml", pathPrefix: prefix.trim() || "ontology/", limit: 50 });
    if (!found.ok) { setLookupError(found.error ? { error: found.error, cli: found.cli } : null); return; }
    const out: OntologyFile[] = [];
    for (const hit of namesFrom(found.data).filter((h) => /\.ya?ml$/u.test(h.name)).slice(0, 30)) {
      const cat = await runCli("cli-query", { op: "knowledge-cat", path: hit.name });
      const content = (cat.data as { content?: unknown } | null)?.content;
      if (cat.ok && typeof content === "string") out.push({ id: `g:${hit.name}`, name: hit.name, group: "gateway", text: content });
    }
    setGatewayFiles(out);
  }, [prefix]);

  const copy = async () => {
    if (!selected) return;
    if (selected.text.length > COPY_LIMIT) { setTooLarge(true); return; }
    setTooLarge(false);
    try { await copyText(selected.text); setCopied(true); window.setTimeout(() => setCopied(false), 1600); } catch { setTooLarge(true); }
  };
  const doExport = async () => {
    setExportFailed(false);
    const picked = await invoke("pick-path", { purpose: "export-dir" });
    if (!picked.ok || !("grantId" in picked.value)) return;
    const result = await invoke("cli-ontology-export", { grantId: picked.value.grantId, operationId: createOperationId("exp"), files: files.map((f) => ({ name: f.group === "gateway" ? `gateway/${f.name.replace(/^\/+/u, "")}` : f.name, text: f.text })) });
    if (result.ok) setExported(result.value); else setExportFailed(true);
  };

  if (!versions) return <Skeleton height={200} />;
  return (
    <div className="so-files" data-testid="ontology-files">
      {failed && <Notice tone="warning" title={t("app.files.versionsFailed")} testId="ontology-files-versions-error">{t("app.files.versionsFailed.body")}</Notice>}
      <div className="so-split so-files-split">
        <Card className="so-files-list">
          {files.length === 0 && <EmptyState icon={FileText} title={t("app.files.empty")} description={t("app.files.emptyBody")} />}
          {(["version", "gateway"] as const).map((group) => {
            const rows = files.filter((f) => f.group === group);
            if (rows.length === 0) return null;
            return (
              <Fragment key={group}>
                <p className="so-files-group">{t(group === "version" ? "app.files.group.versions" : "app.files.group.gateway")}</p>
                <ul className="so-query-names" aria-label={t(group === "version" ? "app.files.group.versions" : "app.files.group.gateway")}>
                  {rows.map((f) => <li key={f.id}><button type="button" className="so-list-row" aria-current={selectedId === f.id ? "true" : undefined} onClick={() => { setSelectedId(f.id); setCompareId((c) => (c === f.id ? "" : c)); }} data-testid="ontology-file-row" data-name={f.name}><span className="so-list-lead"><FileText size={16} strokeWidth={1.5} aria-hidden="true" /></span><span className="so-list-text"><span className="so-list-title so-truncate so-mono">{f.name}</span></span></button></li>)}
                </ul>
              </Fragment>
            );
          })}
          <div className="so-files-gateway">
            <label className="so-field-label" htmlFor="ontology-files-prefix">{t("app.files.gateway.prefix")}</label>
            <div className="so-toolbar"><input id="ontology-files-prefix" className="so-input" value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="ontology/" spellCheck={false} data-testid="ontology-files-prefix" /><Button size="compact" variant="secondary" onClick={() => { void loadGateway(); }} data-testid="ontology-files-gateway-load">{t("app.files.gateway.load")}</Button></div>
            {lookupError && <CliErrorNotice error={lookupError.error} cli={lookupError.cli} title={t("app.files.gateway.failed")} testId="ontology-files-gateway-error" />}
          </div>
        </Card>
        <Card className="so-files-viewer">
          {!selected ? <EmptyState icon={FileText} title={t("app.files.pick")} description={t("app.files.pickBody")} /> : (
            <>
              <div className="so-toolbar so-files-head">
                <h3 className="so-card-title so-mono so-truncate" data-testid="ontology-file-title">{selected.name}</h3>
                <span className="so-toolbar-spacer" />
                <Select label={t("app.files.compare")} labelHidden value={compareId} onChange={(e) => setCompareId(e.target.value)} data-testid="ontology-file-compare">
                  <option value="">{t("app.files.compare.none")}</option>
                  {files.filter((f) => f.id !== selected.id && f.group === selected.group).map((f) => <option key={f.id} value={f.id}>{t("app.files.compare.with", { name: f.name })}</option>)}
                </Select>
                <Button size="compact" variant="ghost" icon={copied ? Check : Copy} onClick={() => { void copy(); }} data-testid="ontology-file-copy">{copied ? t("app.common.copied") : t("app.common.copy")}</Button>
                <Button size="compact" variant="secondary" icon={Download} onClick={() => { void doExport(); }} data-testid="ontology-file-export">{t("app.files.export")}</Button>
              </div>
              {tooLarge && <Notice tone="warning" title={t("app.query.copyTooLarge")}>{t("app.files.copyTooLarge.body")}</Notice>}
              {exported && <Notice tone="success" title={t("app.files.exported", { count: exported.count })} testId="ontology-file-exported"><span className="so-mono so-files-dir">{exported.directory}</span><CliCommand command={exported.cli} testId="ontology-file-export-cli" /></Notice>}
              {exportFailed && <Notice tone="danger" title={t("app.files.exportFailed")} />}
              {diff ? (
                <>
                  <p className="so-weak so-files-diff-note"><GitCompare size={14} strokeWidth={1.5} aria-hidden="true" /> {t("app.files.diff", { from: compare?.name ?? "", to: selected.name, count: diff.filter((l) => l.kind !== "same").length })}</p>
                  <DiffView lines={diff} name={t("app.files.compare")} />
                </>
              ) : <YamlView text={selected.text} name={selected.name} />}
              <p className="so-weak so-files-readonly">{t("app.files.readonly")}</p>
            </>
          )}
        </Card>
      </div>
    </div>
  );
}
