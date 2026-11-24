import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, EmptyState, Notice, Segmented, Select, Skeleton, TextArea, TextField } from "@open-ontology/design-system";
import { Check, Copy, FileText, FolderOpen, Play, Plug, Search, ShieldCheck, Table2 } from "lucide-react";
import type { CliResult, GatewayState, QueryRequest } from "../../../shared/super-ontology/bridge";
import { invoke } from "../../model/bridgeInvoke";
import { COPY_LIMIT, runCli } from "../../model/cliApi";
import { copyText } from "../../model/format";
import { queryDraft } from "../../model/queryDraft";
import { displayCell, namesFrom, toCsv, toJsonRows, toQueryTable, type QueryTable } from "../../model/cliFormat";
import { CliCommand, CliErrorNotice } from "../shared/Cli";

type Mode = "sql" | "tables" | "knowledge";
const emptyGateway: GatewayState = { configured: false, url: null, transport: "production", tokenFile: null, caFile: null };

/** Ontology > Query: `oo query|validate|tables|describe|knowledge` against the connected Gateway, read-only (the Gateway refuses writes). */
export function QueryPanel() {
  const { t } = useLocalization();
  const [gateway, setGateway] = useState<GatewayState | null>(null);
  const [mode, setMode] = useState<Mode>("sql");
  const load = useCallback(async () => { const result = await invoke("cli-gateway-read", {}); setGateway(result.ok ? result.value : emptyGateway); }, []);
  useEffect(() => { void load(); }, [load]);
  if (!gateway) return <Skeleton height={160} />;
  return (
    <div className="so-query" data-testid="ontology-query">
      <GatewayBar gateway={gateway} onChanged={setGateway} />
      {gateway.configured && (
        <>
          <div className="so-query-mode"><Segmented<Mode> label={t("app.query.mode")} value={mode} onChange={setMode} options={[{ value: "sql", label: t("app.query.mode.sql"), testId: "query-mode-sql" }, { value: "tables", label: t("app.query.mode.tables"), testId: "query-mode-tables" }, { value: "knowledge", label: t("app.query.mode.knowledge"), testId: "query-mode-knowledge" }]} /></div>
          {mode === "sql" && <SqlRunner />}
          {mode === "tables" && <Tables />}
          {mode === "knowledge" && <Knowledge />}
        </>
      )}
    </div>
  );
}

function GatewayBar({ gateway, onChanged }: { gateway: GatewayState; onChanged: (state: GatewayState) => void }) {
  const { t } = useLocalization();
  const [url, setUrl] = useState("http://127.0.0.1:8443");
  const [transport, setTransport] = useState<"production" | "local-development">("local-development");
  const [token, setToken] = useState<{ id: string; path: string } | null>(null);
  const [ca, setCa] = useState<{ id: string; path: string } | null>(null);
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<string | null>(null);
  const pick = async (purpose: "token-file" | "ca-file", set: (value: { id: string; path: string }) => void) => {
    const picked = await invoke("pick-path", { purpose });
    if (picked.ok && "grantId" in picked.value) set({ id: picked.value.grantId, path: picked.value.displayPath });
  };
  const connect = async () => {
    setTouched(true);
    if (!token || !/^https?:\/\/[^/\s]+\/?$/u.test(url.trim())) return;
    setBusy(true); setFailure(null);
    const result = await invoke("cli-gateway-set", { url: url.trim(), tokenGrantId: token.id, ...(ca ? { caGrantId: ca.id } : {}), transport });
    setBusy(false);
    if (result.ok) onChanged(result.value); else setFailure(result.error.code);
  };
  if (gateway.configured) {
    return (
      <div className="so-query-gateway" data-testid="query-gateway" data-state="connected">
        <ShieldCheck size={16} strokeWidth={1.5} aria-hidden="true" />
        <span className="so-truncate">{t("app.query.gateway.connected", { url: gateway.url ?? "" })}</span>
        <Button size="compact" variant="ghost" onClick={() => { void invoke("cli-gateway-clear", {}).then(() => onChanged(emptyGateway)); }} data-testid="query-gateway-disconnect">{t("app.query.gateway.change")}</Button>
      </div>
    );
  }
  return (
    <Card className="so-query-connect" data-testid="query-gateway" data-state="disconnected">
      <h3 className="so-card-title">{t("app.query.gateway.title")}</h3>
      <p className="so-weak">{t("app.query.gateway.body")}</p>
      {failure && <Notice tone="danger" title={t("app.query.gateway.failed")} testId="query-gateway-error">{failure}</Notice>}
      <TextField label={t("app.query.gateway.url")} value={url} onChange={(e) => setUrl(e.target.value)} error={touched && !/^https?:\/\/[^/\s]+\/?$/u.test(url.trim()) ? t("app.query.gateway.url.bad") : null} spellCheck={false} data-testid="query-gateway-url" />
      <Select label={t("app.query.gateway.transport")} value={transport} onChange={(e) => setTransport(e.target.value as typeof transport)} helper={t("app.query.gateway.transport.help")} data-testid="query-gateway-transport">
        <option value="production">{t("app.query.gateway.transport.production")}</option><option value="local-development">{t("app.query.gateway.transport.local")}</option>
      </Select>
      <div className="so-db-ca-file"><Button variant="secondary" icon={FolderOpen} onClick={() => { void pick("token-file", setToken); }} data-testid="query-gateway-token">{t("app.query.gateway.token")}</Button><span className="so-mono so-truncate" title={token?.path}>{token?.path ?? (touched ? t("app.db.required") : t("app.db.ca.none"))}</span></div>
      <div className="so-db-ca-file"><Button variant="secondary" icon={FolderOpen} onClick={() => { void pick("ca-file", setCa); }} data-testid="query-gateway-ca">{t("app.query.gateway.ca")}</Button><span className="so-mono so-truncate" title={ca?.path}>{ca?.path ?? t("app.db.optional")}</span></div>
      <div><Button variant="primary" icon={Plug} loading={busy} onClick={() => { void connect(); }} data-testid="query-gateway-connect">{t("app.query.gateway.connect")}</Button></div>
      <CliCommand command="oo tables --json --gateway https://gateway.example --token-file ~/.config/open-ontology/token" testId="query-gateway-cli" />
    </Card>
  );
}

/** One Gateway call with its latest answer. */
function useQuery() {
  const [busy, setBusy] = useState(false);
  const [result, setResult] = useState<CliResult | null>(null);
  const seq = useRef(0);
  const run = useCallback(async (request: QueryRequest) => {
    const mine = ++seq.current;
    setBusy(true);
    const value = await runCli("cli-query", request);
    if (mine === seq.current) { setResult(value); setBusy(false); }
    return value;
  }, []);
  return { busy, result, run, clear: () => setResult(null) };
}

function SqlRunner() {
  const { t } = useLocalization();
  const [sql, setSql] = useState(() => queryDraft.take() ?? "");
  const [maxRows, setMaxRows] = useState("500");
  const [validated, setValidated] = useState<CliResult | null>(null);
  const [mode, setMode] = useState<"validate" | "query" | null>(null);
  const q = useQuery();
  const empty = sql.trim() === "";
  const execute = async (op: "validate" | "query") => {
    if (empty) return;
    setMode(op);
    if (op === "validate") { const value = await q.run({ op, sql }); setValidated(value); }
    else { setValidated(null); await q.run({ op, sql, maxRows: Number(maxRows) }); }
  };
  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") { event.preventDefault(); void execute("query"); } };
  const table = q.result?.ok && mode === "query" ? toQueryTable(q.result.data) : null;
  return (
    <div className="so-query-sql" data-testid="query-sql">
      <TextArea label={t("app.query.sql")} value={sql} onChange={(e) => { setSql(e.target.value); setValidated(null); }} onKeyDown={onKey} rows={6} placeholder="select * from public.customers limit 10" spellCheck={false} className="so-query-editor" helper={t("app.query.sql.help")} data-testid="query-sql-input" />
      <div className="so-toolbar">
        <Button variant="secondary" icon={ShieldCheck} loading={q.busy && mode === "validate"} disabled={empty || (q.busy && mode !== "validate")} onClick={() => { void execute("validate"); }} data-testid="query-validate">{t("app.query.validate")}</Button>
        <Button variant="primary" icon={Play} loading={q.busy && mode === "query"} disabled={empty || (q.busy && mode !== "query")} onClick={() => { void execute("query"); }} data-testid="query-run">{t("app.query.run")}</Button>
        <Select label={t("app.query.maxRows")} labelHidden value={maxRows} onChange={(e) => setMaxRows(e.target.value)} data-testid="query-max-rows">{["100", "500", "1000", "2000"].map((n) => <option key={n} value={n}>{t("app.query.rows", { count: Number(n) })}</option>)}</Select>
      </div>
      {mode === "validate" && validated?.ok && <Notice tone="success" title={t("app.query.valid")} testId="query-valid"><CliCommand command={validated.cli} testId="query-valid-cli" /></Notice>}
      {q.result && !q.result.ok && q.result.error && <CliErrorNotice error={q.result.error} cli={q.result.cli} title={mode === "validate" ? t("app.query.invalid") : t("app.query.failed")} testId="query-error" />}
      {table && <ResultTable table={table} cli={q.result?.cli ?? ""} />}
      {q.result?.ok && mode === "query" && !table && <Notice tone="warning" title={t("app.query.unreadable")}>{t("app.query.unreadable.body")}</Notice>}
    </div>
  );
}

export function ResultTable({ table, cli }: { table: QueryTable; cli: string }) {
  const { t } = useLocalization();
  const csv = useMemo(() => toCsv(table), [table]);
  const json = useMemo(() => toJsonRows(table), [table]);
  const [copied, setCopied] = useState<"csv" | "json" | null>(null);
  const [failed, setFailed] = useState(false);
  const timer = useRef<number | null>(null);
  useEffect(() => () => { if (timer.current !== null) window.clearTimeout(timer.current); }, []);
  const copy = async (kind: "csv" | "json") => {
    const text = kind === "csv" ? csv : json;
    if (text.length > COPY_LIMIT) { setFailed(true); return; }
    setFailed(false);
    try { await copyText(text); setCopied(kind); if (timer.current !== null) window.clearTimeout(timer.current); timer.current = window.setTimeout(() => setCopied(null), 1600); } catch { setFailed(true); }
  };
  return (
    <div className="so-query-result" data-testid="query-result" data-rows={table.rows.length}>
      <div className="so-toolbar so-query-result-head">
        <span className="so-weak" data-testid="query-result-summary">{t("app.query.result", { count: table.returned, ms: table.durationMs ?? 0 })}</span>
        <span className="so-toolbar-spacer" />
        <Button size="compact" variant="ghost" icon={copied === "csv" ? Check : Copy} onClick={() => { void copy("csv"); }} data-testid="query-copy-csv">{copied === "csv" ? t("app.common.copied") : t("app.query.copyCsv")}</Button>
        <Button size="compact" variant="ghost" icon={copied === "json" ? Check : Copy} onClick={() => { void copy("json"); }} data-testid="query-copy-json">{copied === "json" ? t("app.common.copied") : t("app.query.copyJson")}</Button>
      </div>
      {table.truncated && <Notice tone="warning" title={t("app.query.truncated")} testId="query-truncated">{t("app.query.truncated.body")}</Notice>}
      {failed && <Notice tone="warning" title={t("app.query.copyTooLarge")} testId="query-copy-large">{t("app.query.copyTooLarge.body")}</Notice>}
      {table.warnings.map((warning) => <Notice key={warning} tone="warning" title={t("app.query.warning")}>{warning}</Notice>)}
      <div className="so-query-scroll" tabIndex={0} role="region" aria-label={t("app.query.resultTable")}>
        <table className="so-table so-query-table">
          <thead><tr>{table.columns.map((column) => <th key={column.name} scope="col"><span className="so-truncate">{column.name}</span>{column.type && <span className="so-query-type so-mono">{column.type}</span>}</th>)}</tr></thead>
          <tbody>{table.rows.map((row, r) => <tr key={r}>{table.columns.map((column, c) => { const text = displayCell(row[c]); return <td key={column.name + c} title={text} data-null={row[c] === null || row[c] === undefined ? "true" : undefined}><span className="so-truncate">{text === "" && (row[c] === null || row[c] === undefined) ? "NULL" : text}</span></td>; })}</tr>)}</tbody>
        </table>
      </div>
      <CliCommand command={cli} testId="query-cli" />
    </div>
  );
}

function Tables() {
  const { t } = useLocalization();
  const list = useQuery();
  const detail = useQuery();
  const [filter, setFilter] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  useEffect(() => { void list.run({ op: "tables" }); }, []); // eslint-disable-line react-hooks/exhaustive-deps
  const names = useMemo(() => namesFrom(list.result?.data).filter((n) => n.name.toLowerCase().includes(filter.toLowerCase())), [list.result, filter]);
  const columns = useMemo(() => {
    const data = detail.result?.data as { columns?: unknown[]; column_details?: { name: string; type?: string }[] } | null;
    if (!data?.columns) return [];
    return data.columns.map((c) => { const name = String(c); return { name, type: data.column_details?.find((d) => d.name === name)?.type ?? "" }; });
  }, [detail.result]);
  const open = (name: string) => { setSelected(name); void detail.run({ op: "describe", table: name }); };
  return (
    <div className="so-split so-query-tables" data-testid="query-tables">
      <Card className="so-query-tables-list">
        <div className="so-search"><Search className="so-search-icon" size={16} strokeWidth={1.5} aria-hidden="true" /><input className="so-input" type="search" value={filter} onChange={(e) => setFilter(e.target.value)} placeholder={t("app.query.tables.filter")} aria-label={t("app.query.tables.filter")} data-testid="query-tables-filter" /></div>
        {list.busy && !list.result && <Skeleton height={120} />}
        {list.result && !list.result.ok && list.result.error && <CliErrorNotice error={list.result.error} cli={list.result.cli} title={t("app.query.tables.failed")} testId="query-tables-error" />}
        {list.result?.ok && names.length === 0 && <EmptyState icon={Table2} title={t("app.query.tables.empty")} description={t("app.query.tables.emptyBody")} />}
        <ul className="so-query-names" aria-label={t("app.query.mode.tables")}>{names.map((n) => <li key={n.name}><button type="button" className="so-list-row" aria-current={selected === n.name ? "true" : undefined} onClick={() => open(n.name)} data-testid="query-table-row" data-name={n.name}><span className="so-list-lead"><Table2 size={16} strokeWidth={1.5} aria-hidden="true" /></span><span className="so-list-text"><span className="so-list-title so-truncate so-mono">{n.name}</span>{n.detail && <span className="so-list-sub so-truncate">{n.detail}</span>}</span></button></li>)}</ul>
        {list.result && <CliCommand command={list.result.cli} testId="query-tables-cli" />}
      </Card>
      <Card className="so-query-describe" data-testid="query-describe">
        {!selected && <EmptyState icon={Table2} title={t("app.query.describe.empty")} description={t("app.query.describe.emptyBody")} />}
        {selected && <><h3 className="so-card-title so-mono">{selected}</h3>
          {detail.busy && <Skeleton height={96} />}
          {detail.result && !detail.result.ok && detail.result.error && <CliErrorNotice error={detail.result.error} cli={detail.result.cli} title={t("app.query.describe.failed")} />}
          {columns.length > 0 && <table className="so-table"><thead><tr><th scope="col">{t("app.query.describe.column")}</th><th scope="col">{t("app.query.describe.type")}</th></tr></thead><tbody>{columns.map((c) => <tr key={c.name}><td><span className="so-truncate so-mono">{c.name}</span></td><td><span className="so-truncate so-mono">{c.type || "-"}</span></td></tr>)}</tbody></table>}
          {detail.result && <CliCommand command={detail.result.cli} testId="query-describe-cli" />}</>}
      </Card>
    </div>
  );
}

function Knowledge() {
  const { t } = useLocalization();
  const search = useQuery();
  const cat = useQuery();
  const [text, setText] = useState("");
  const [prefix, setPrefix] = useState("");
  const [opened, setOpened] = useState<string | null>(null);
  const hits = useMemo(() => namesFrom(search.result?.data), [search.result]);
  const doSearch = (event?: { preventDefault: () => void }) => { event?.preventDefault(); if (text.trim() === "") return; setOpened(null); void search.run({ op: "knowledge-search", text: text.trim(), ...(prefix.trim() ? { pathPrefix: prefix.trim() } : {}), limit: 20 }); };
  const open = (path: string) => { setOpened(path); void cat.run({ op: "knowledge-cat", path }); };
  const body = typeof (cat.result?.data as { content?: unknown } | null)?.content === "string" ? (cat.result?.data as { content: string }).content : cat.result?.data ? JSON.stringify(cat.result.data, null, 2) : "";
  return (
    <div className="so-split so-query-knowledge" data-testid="query-knowledge">
      <Card className="so-query-tables-list">
        <form className="so-query-search" onSubmit={doSearch} role="search">
          <TextField label={t("app.query.knowledge.search")} value={text} onChange={(e) => setText(e.target.value)} placeholder={t("app.query.knowledge.placeholder")} helper={t("app.query.knowledge.help")} spellCheck={false} data-testid="query-knowledge-input" />
          <TextField label={t("app.query.knowledge.prefix")} optionalLabel={t("app.db.optional")} value={prefix} onChange={(e) => setPrefix(e.target.value)} placeholder="tables/" spellCheck={false} data-testid="query-knowledge-prefix" />
          <div><Button type="submit" variant="primary" icon={Search} loading={search.busy} disabled={text.trim() === ""} data-testid="query-knowledge-run">{t("app.query.knowledge.run")}</Button></div>
        </form>
        {search.result && !search.result.ok && search.result.error && <CliErrorNotice error={search.result.error} cli={search.result.cli} title={t("app.query.knowledge.failed")} testId="query-knowledge-error" />}
        {search.result?.ok && hits.length === 0 && <EmptyState icon={Search} title={t("app.query.knowledge.none")} description={t("app.query.knowledge.noneBody")} />}
        <ul className="so-query-names" aria-label={t("app.query.mode.knowledge")}>{hits.map((hit) => <li key={hit.name}><button type="button" className="so-list-row" aria-current={opened === hit.name ? "true" : undefined} onClick={() => open(hit.name)} data-testid="query-knowledge-hit" data-name={hit.name}><span className="so-list-lead"><FileText size={16} strokeWidth={1.5} aria-hidden="true" /></span><span className="so-list-text"><span className="so-list-title so-truncate so-mono">{hit.name}</span>{hit.detail && <span className="so-list-sub so-truncate">{hit.detail}</span>}</span></button></li>)}</ul>
        {search.result && <CliCommand command={search.result.cli} testId="query-knowledge-cli" />}
      </Card>
      <Card className="so-query-cat" data-testid="query-cat">
        {!opened && <EmptyState icon={FileText} title={t("app.query.cat.empty")} description={t("app.query.cat.emptyBody")} />}
        {opened && <><h3 className="so-card-title so-mono so-truncate">{opened}</h3>
          {cat.busy && <Skeleton height={120} />}
          {cat.result && !cat.result.ok && cat.result.error && <CliErrorNotice error={cat.result.error} cli={cat.result.cli} title={t("app.query.cat.failed")} />}
          {cat.result?.ok && <pre className="so-db-log so-mono so-query-cat-body" tabIndex={0} data-testid="query-cat-body">{body}</pre>}
          {cat.result && <CliCommand command={cat.result.cli} testId="query-cat-cli" />}</>}
      </Card>
    </div>
  );
}
