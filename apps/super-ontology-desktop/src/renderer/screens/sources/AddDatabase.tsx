import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Dialog, Notice, Select, TextArea, TextField } from "@open-ontology/design-system";
import { useErrorText } from "../../hooks/useText";
import { DEFAULT_PORT, databaseApi, invalidDbFields, parseTables, type DbField, type DbKind } from "../../model/databaseApi";
import type { SuperDriveError } from "../../model/superDriveErrors";
import { toSuperDriveError } from "../../model/superDriveErrors";

/** Sources › Add source › Database: the form behind `oo source add postgres|mysql`. The password goes to the app once and is never shown again. */
export function DatabaseDialog({ onClose, onCreated }: { onClose: () => void; onCreated: (name: string) => void }) {
  const { t } = useLocalization();
  const errorText = useErrorText();
  const [kind, setKind] = useState<DbKind>("postgres");
  const [values, setValues] = useState({ name: "", host: "", port: String(DEFAULT_PORT.postgres), database: "", user: "", password: "", caPem: "", tables: "" });
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const set = (patch: Partial<typeof values>) => setValues((current) => ({ ...current, ...patch }));
  const invalid = invalidDbFields(values);
  const problem = (field: DbField) => (touched && invalid[field] ? t(invalid[field] === "required" ? "app.sources.cloud.required" : "app.sources.db.invalid") : null);

  const submit = async () => {
    setTouched(true);
    if (Object.keys(invalid).length > 0) return;
    setBusy(true);
    setError(null);
    const result = await databaseApi.add({ kind, name: values.name.trim(), host: values.host.trim(), port: Number(values.port), database: values.database.trim(), user: values.user.trim(), password: values.password, caPem: values.caPem.trim(), include: parseTables(values.tables) });
    setBusy(false);
    if (!result.ok) { setError(toSuperDriveError(result)); return; }
    onCreated(result.value.name);
  };

  return (
    <Dialog title={t("app.sources.db.title")} description={t("app.sources.db.description")} onClose={busy ? undefined : onClose} closeLabel={t("app.common.close")} testId="database-dialog"
      footer={<><Button variant="ghost" onClick={onClose} disabled={busy} data-testid="add-source-cancel">{t("app.common.cancel")}</Button><Button variant="primary" loading={busy} onClick={() => { void submit(); }} data-testid="add-source-confirm">{t("app.sources.db.connect")}</Button></>}>
      {error && <Notice tone="danger" title={t("app.sources.db.failed")} testId="add-source-error">{errorText(error)}</Notice>}
      <Select label={t("app.sources.db.kind")} value={kind} onChange={(event) => { const next = event.target.value as DbKind; setKind(next); if (values.port === String(DEFAULT_PORT[kind])) set({ port: String(DEFAULT_PORT[next]) }); }} data-testid="db-kind">
        <option value="postgres">PostgreSQL</option>
        <option value="mysql">MySQL</option>
      </Select>
      <TextField data-testid="db-name" label={t("app.sources.cloud.name")} value={values.name} onChange={(e) => set({ name: e.target.value })} error={problem("name")} helper={t("app.sources.db.name.help")} placeholder={t("app.sources.db.name.placeholder")} autoFocus maxLength={64} spellCheck={false} />
      <div className="so-grid" data-cols="2">
        <TextField data-testid="db-host" label={t("app.sources.db.host")} value={values.host} onChange={(e) => set({ host: e.target.value })} error={problem("host")} helper={t("app.sources.db.host.help")} spellCheck={false} />
        <TextField data-testid="db-port" label={t("app.sources.db.port")} value={values.port} onChange={(e) => set({ port: e.target.value })} error={problem("port")} inputMode="numeric" spellCheck={false} />
      </div>
      <TextField data-testid="db-database" label={t("app.sources.db.database")} value={values.database} onChange={(e) => set({ database: e.target.value })} error={problem("database")} spellCheck={false} />
      <div className="so-grid" data-cols="2">
        <TextField data-testid="db-user" label={t("app.sources.db.user")} value={values.user} onChange={(e) => set({ user: e.target.value })} error={problem("user")} helper={t("app.sources.db.user.help")} spellCheck={false} autoComplete="off" />
        <TextField data-testid="db-password" label={t("app.sources.db.password")} type="password" value={values.password} onChange={(e) => set({ password: e.target.value })} error={problem("password")} helper={t("app.sources.db.password.help")} autoComplete="off" />
      </div>
      <TextArea data-testid="db-ca" label={t("app.sources.db.ca")} value={values.caPem} onChange={(e) => set({ caPem: e.target.value })} error={problem("caPem")} helper={t("app.sources.db.ca.help")} placeholder="-----BEGIN CERTIFICATE-----" spellCheck={false} rows={4} className="so-field-mono" />
      <TextArea data-testid="db-tables" label={t("app.sources.db.tables")} value={values.tables} onChange={(e) => set({ tables: e.target.value })} error={problem("include")} helper={t("app.sources.db.tables.help")} placeholder="public.orders" spellCheck={false} rows={3} className="so-field-mono" />
    </Dialog>
  );
}
