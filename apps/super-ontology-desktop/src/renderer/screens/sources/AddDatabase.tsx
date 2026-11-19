import { useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Dialog, Notice, Segmented, Select, StatusDot, TextArea, TextField } from "@open-ontology/design-system";
import { FolderOpen, PlugZap } from "lucide-react";
import { DB_KINDS, HOST_NAME, SOURCE_NAME, TABLE_ENTRY, dbKind } from "../../../shared/super-ontology/db-kinds.mjs";
import type { CliResult, DbConnectionFields } from "../../../shared/super-ontology/bridge";
import { invoke } from "../../model/bridgeInvoke";
import { runCli, withOperation } from "../../model/cliApi";
import { CliCommand, CliErrorNotice } from "../shared/Cli";
import { useApp } from "../../hooks/useApp";

type Form = { kind: string; name: string; host: string; port: string; database: string; user: string; password: string; caMode: "paste" | "file"; caPem: string; caGrant: { id: string; path: string } | null; include: string; exclude: string };
const EMPTY: Form = { kind: "postgres", name: "", host: "", port: "", database: "", user: "", password: "", caMode: "paste", caPem: "", caGrant: null, include: "", exclude: "" };
const lines = (text: string) => text.split(/[\n,]/u).map((line) => line.trim()).filter(Boolean);
type Touched = "name" | "host" | "port" | "database" | "user" | "password" | "ca" | "include";

export function validateForm(form: Form): Touched[] {
  const kind = dbKind(form.kind);
  const bad: Touched[] = [];
  if (!SOURCE_NAME.test(form.name)) bad.push("name");
  if (!HOST_NAME.test(form.host)) bad.push("host");
  if (form.port !== "" && !(/^[0-9]{1,5}$/u.test(form.port) && Number(form.port) >= 1 && Number(form.port) <= 65535)) bad.push("port");
  if (form.database.trim() === "") bad.push("database");
  if (form.user.trim() === "") bad.push("user");
  if (form.password === "") bad.push("password");
  if (kind?.tls === "required" && !(form.caMode === "paste" ? form.caPem.trim() !== "" : form.caGrant !== null)) bad.push("ca");
  const include = lines(form.include);
  if (include.length === 0 || !include.every((entry) => TABLE_ENTRY.test(entry))) bad.push("include");
  return bad;
}

const CHECK_LABEL = (t: (id: string) => string, name: string): string => (
  name === "resolve" ? t("app.db.check.resolve") : name === "port" ? t("app.db.check.port") : name === "tls" ? t("app.db.check.tls") : name === "ca_file" ? t("app.db.check.caFile") : name === "sign_in" ? t("app.db.check.signIn") : name
);

/** Add source > Database (D88): the CLI's own `oo init` check and `oo source add` do the work; this form only collects the fields. */
export function AddDatabaseDialog({ onClose, onAdded }: { onClose: () => void; onAdded: (name: string) => void }) {
  const { t } = useLocalization();
  const app = useApp();
  const [form, setForm] = useState<Form>(EMPTY);
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState<"check" | "add" | null>(null);
  const [result, setResult] = useState<{ kind: "check" | "add"; value: CliResult<{ checks?: { name: string; status: string; detail: string }[] }> } | null>(null);
  const kind = dbKind(form.kind);
  const bad = useMemo(() => new Set(validateForm(form)), [form]);
  const set = (patch: Partial<Form>) => { setForm((current) => ({ ...current, ...patch })); setResult(null); };
  const required = t("app.db.required");
  const err = (field: Touched, message = required) => (touched && bad.has(field) ? message : null);
  const unwired = DB_KINDS.filter((k) => !k.wired);
  const nameLabel = kind?.nameField === "service" ? t("app.db.service") : t("app.db.database");

  const fields = (): DbConnectionFields => ({
    kind: form.kind, host: form.host.trim(), ...(form.port ? { port: Number(form.port) } : {}), database: form.database.trim(), user: form.user.trim(), password: form.password,
    ...(form.caMode === "paste" && form.caPem.trim() ? { tlsCaPem: form.caPem.trim() } : {}), ...(form.caMode === "file" && form.caGrant ? { tlsCaGrantId: form.caGrant.id } : {}),
  });
  const connectionBad = (["host", "port", "database", "user", "password", "ca"] as Touched[]).some((field) => bad.has(field));

  const chooseCa = async () => {
    const picked = await invoke("pick-path", { purpose: "ca-file" });
    if (picked.ok && "grantId" in picked.value) set({ caGrant: { id: picked.value.grantId, path: picked.value.displayPath } });
  };

  const test = async () => {
    setTouched(true);
    if (connectionBad) return;
    setBusy("check");
    const value = await runCli("cli-db-check", fields());
    setBusy(null);
    setResult({ kind: "check", value });
  };
  const submit = async () => {
    setTouched(true);
    if (bad.size > 0) return;
    setBusy("add");
    const value = await runCli("cli-db-add", withOperation("db", { ...fields(), name: form.name, include: lines(form.include), ...(lines(form.exclude).length ? { exclude: lines(form.exclude) } : {}) }));
    setBusy(null);
    if (value.ok) { onAdded(form.name); return; }
    setResult({ kind: "add", value: value as CliResult<{ checks?: { name: string; status: string; detail: string }[] }> });
  };

  const failure = result && !result.value.ok ? result.value : null;
  const checks = result?.kind === "check" && result.value.ok ? result.value.data?.checks ?? [] : result?.kind === "check" ? result.value.data?.checks ?? [] : [];
  return (
    <Dialog title={t("app.db.title")} description={t("app.db.description")} onClose={busy ? undefined : onClose} closeLabel={t("app.common.close")} size="lg"
      footer={<>
        <Button variant="ghost" onClick={onClose} disabled={busy !== null} data-testid="add-database-cancel">{t("app.common.cancel")}</Button>
        {kind?.preflight && <Button variant="secondary" icon={PlugZap} loading={busy === "check"} disabled={busy === "add"} onClick={() => { void test(); }} data-testid="add-database-test">{t("app.db.test")}</Button>}
        <Button variant="primary" loading={busy === "add"} disabled={busy === "check"} onClick={() => { void submit(); }} data-testid="add-database-confirm">{t("app.db.add")}</Button>
      </>}>
      {failure?.error && <CliErrorNotice error={failure.error} cli={failure.cli} title={result?.kind === "check" ? t("app.db.check.failed") : t("app.db.add.failed")} testId="add-database-error" />}
      {result?.kind === "check" && result.value.ok && <Notice tone="success" title={t("app.db.check.ok")} testId="add-database-check-ok">{t("app.db.check.okBody")}</Notice>}
      <Select label={t("app.db.kind")} value={form.kind} onChange={(event) => set({ kind: event.target.value, port: "" })} data-testid="add-database-kind">
        <optgroup label={t("app.db.kind.ready")}>{DB_KINDS.filter((k) => k.wired).map((k) => <option key={k.id} value={k.id}>{k.label}</option>)}</optgroup>
        <optgroup label={t("app.db.kind.pending")}>{unwired.map((k) => <option key={k.id} value={k.id} disabled>{k.label}</option>)}</optgroup>
      </Select>
      <p className="so-weak so-db-pending" data-testid="add-database-pending">{t("app.db.kind.pendingNote", { kinds: unwired.map((k) => k.label).join(", ") })}</p>
      <TextField label={t("app.db.name")} value={form.name} onChange={(e) => set({ name: e.target.value })} error={err("name", form.name === "" ? required : t("app.db.name.bad"))} placeholder={t("app.db.name.placeholder")} helper={t("app.db.name.help")} autoFocus maxLength={64} spellCheck={false} data-testid="add-database-name" />
      <div className="so-grid" data-cols="2">
        <TextField label={t("app.db.host")} value={form.host} onChange={(e) => set({ host: e.target.value })} error={err("host", form.host === "" ? required : t("app.db.host.bad"))} placeholder="db.example.test" spellCheck={false} data-testid="add-database-host" />
        <TextField label={t("app.db.port")} value={form.port} onChange={(e) => set({ port: e.target.value })} error={err("port", t("app.db.port.bad"))} placeholder={String(kind?.port ?? "")} inputMode="numeric" optionalLabel={t("app.db.optional")} data-testid="add-database-port" />
      </div>
      <TextField label={nameLabel} value={form.database} onChange={(e) => set({ database: e.target.value })} error={err("database")} spellCheck={false} data-testid="add-database-database" />
      <div className="so-grid" data-cols="2">
        <TextField label={t("app.db.user")} value={form.user} onChange={(e) => set({ user: e.target.value })} error={err("user")} autoComplete="off" spellCheck={false} data-testid="add-database-user" />
        <TextField label={t("app.db.password")} type="password" value={form.password} onChange={(e) => set({ password: e.target.value })} error={err("password")} autoComplete="off" data-testid="add-database-password" />
      </div>
      <p className="so-weak">{t("app.db.password.help")}</p>
      <div className="so-db-ca">
        <div className="so-db-ca-head">
          <span className="so-field-label">{t("app.db.ca")}{kind?.tls === "optional" ? <span className="so-weak"> · {t("app.db.optional")}</span> : null}</span>
          <Segmented<"paste" | "file"> label={t("app.db.ca")} value={form.caMode} onChange={(caMode) => set({ caMode })} options={[{ value: "paste", label: t("app.db.ca.paste") }, { value: "file", label: t("app.db.ca.file") }]} />
        </div>
        {form.caMode === "paste"
          ? <TextArea label={t("app.db.ca.pem")} labelHidden value={form.caPem} onChange={(e) => set({ caPem: e.target.value })} rows={4} placeholder="-----BEGIN CERTIFICATE-----" error={err("ca")} spellCheck={false} data-testid="add-database-ca-pem" />
          : <div className="so-db-ca-file"><Button variant="secondary" icon={FolderOpen} onClick={() => { void chooseCa(); }} data-testid="add-database-ca-choose">{t("app.db.ca.choose")}</Button><span className="so-mono so-truncate" title={form.caGrant?.path}>{form.caGrant?.path ?? (touched && bad.has("ca") ? required : t("app.db.ca.none"))}</span></div>}
        <p className="so-weak">{t("app.db.ca.help")}</p>
      </div>
      <TextArea label={t("app.db.include")} value={form.include} onChange={(e) => set({ include: e.target.value })} rows={3} placeholder={"public.customers\npublic.orders"} error={err("include", t("app.db.include.bad"))} helper={t("app.db.include.help")} spellCheck={false} data-testid="add-database-include" />
      <TextArea label={t("app.db.exclude")} optionalLabel={t("app.db.optional")} value={form.exclude} onChange={(e) => set({ exclude: e.target.value })} rows={2} placeholder="public.audit_log" spellCheck={false} data-testid="add-database-exclude" />
      {checks.length > 0 && (
        <ul className="so-db-checks" data-testid="add-database-checks" aria-label={t("app.db.check.title")}>
          {checks.map((check) => (
            <li key={check.name} data-state={check.status} data-testid="add-database-check">
              <StatusDot tone={check.status === "ok" ? "success" : check.status === "blocked" ? "danger" : "warning"} label={CHECK_LABEL(t, check.name)} />
              <span className="so-mono so-truncate" title={check.detail}>{check.detail}</span>
            </li>
          ))}
        </ul>
      )}
      {result?.value.ok && result.value.cli && <CliCommand command={result.value.cli} testId="add-database-cli" />}
      {!kind?.preflight && <p className="so-weak">{t("app.db.checkAfter")}</p>}
      <p className="so-weak">{t("app.db.stack", { where: app.section === "sources" ? t("app.db.stack.where") : t("app.db.stack.where") })}</p>
    </Dialog>
  );
}
