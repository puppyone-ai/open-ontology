import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, DefinitionList, Dialog, Notice, Section, Select, Skeleton, TextField } from "@open-ontology/design-system";
import { Pencil } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import type { ModelConfig } from "../../../shared/super-ontology/docs-api.generated";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import type { SuperDriveError } from "../../model/superDriveErrors";
import { isEngineReady } from "../../model/superDriveStateModel";

type Provider = "openai-compatible" | "anthropic";
const URL_PATTERN = /^https?:\/\/\S+$/u;

function ModelDialog({ config, onClose }: { config: ModelConfig | null; onClose: () => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const [provider, setProvider] = useState<Provider>(config?.provider ?? "openai-compatible");
  const [baseUrl, setBaseUrl] = useState(config?.base_url ?? "");
  const [model, setModel] = useState(config?.model ?? "");
  const [apiKey, setApiKey] = useState("");
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const keyNeeded = !config?.configured;
  const urlError = baseUrl.trim() === "" ? t("app.model.required") : !URL_PATTERN.test(baseUrl.trim()) ? t("app.model.url.bad") : null;
  const save = async () => {
    setTouched(true);
    if (urlError || model.trim() === "" || (keyNeeded && apiKey.trim() === "")) return;
    setBusy(true); setError(null);
    const result = await actions.saveModelConfig({ provider, baseUrl: baseUrl.trim(), model: model.trim(), ...(apiKey.trim() ? { apiKey: apiKey.trim() } : {}) });
    setBusy(false);
    if (!result.ok) { setError(result.error); return; }
    onClose();
  };
  return (
    <Dialog title={t("app.model.dialog.title")} description={t("app.model.dialog.description")} onClose={busy ? undefined : onClose} closeLabel={t("app.common.close")}
      footer={<><Button variant="ghost" onClick={onClose} disabled={busy}>{t("app.common.cancel")}</Button><Button variant="primary" loading={busy} onClick={() => { void save(); }}>{t("app.model.save")}</Button></>}>
      {error && <Notice tone="danger" title={t("app.model.failed")}>{error.code === "DOCS_ENGINE_PRECONDITION_FAILED" ? t("app.model.stale") : errorText(error)}</Notice>}
      <Select label={t("app.model.provider")} value={provider} onChange={(e) => setProvider(e.target.value as Provider)}>
        <option value="openai-compatible">{t("app.model.provider.openai")}</option>
        <option value="anthropic">Anthropic</option>
      </Select>
      <TextField label={t("app.model.baseUrl")} value={baseUrl} onChange={(e) => setBaseUrl(e.target.value)} placeholder="https://" error={touched ? urlError : null} spellCheck={false} />
      <TextField label={t("app.model.model")} value={model} onChange={(e) => setModel(e.target.value)} error={touched && model.trim() === "" ? t("app.model.required") : null} spellCheck={false} />
      <TextField label={t("app.model.key")} type="password" autoComplete="off" value={apiKey} onChange={(e) => setApiKey(e.target.value)}
        placeholder={config?.configured ? t("app.model.key.saved") : undefined} error={touched && keyNeeded && apiKey.trim() === "" ? t("app.model.required") : null}
        helper={config?.configured ? t("app.model.key.keep") : t("app.model.key.note")} />
    </Dialog>
  );
}

/** Rules read every file; a model is optional and only sees short excerpts of drives that allow it. */
export function ModelSettings() {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const state = useSuperDriveSelector((s) => s.modelConfig);
  const [editing, setEditing] = useState(false);
  useEffect(() => { if (ready) void actions.loadModelConfig(); }, [actions, ready]);
  const config = state.config;
  return (
    <div className="so-stack" data-testid="model-settings">
      <Notice tone="accent" title={t("app.model.optional.title")}>{t("app.model.optional.body")}</Notice>
      <Section title={t("app.model.title")} actions={ready ? <Button icon={Pencil} onClick={() => setEditing(true)}>{config?.configured ? t("app.model.edit") : t("app.model.setup")}</Button> : undefined}>
        {!ready ? <Notice tone="warning" title={t("app.connect.engine.title")}>{t("app.model.engine")}</Notice>
          : state.error && !config ? <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={() => { void actions.loadModelConfig({ force: true }); }}>{t("app.common.retry")}</Button>}>{errorText(state.error)}</Notice>
          : !state.loaded ? <Card className="so-form"><Skeleton height={14} /><Skeleton height={14} width="70%" /></Card>
          : (
            <Card className="so-form">
              {config?.configured ? <DefinitionList items={[
                { label: t("app.model.provider"), value: config.provider === "anthropic" ? "Anthropic" : t("app.model.provider.openai") },
                { label: t("app.model.baseUrl"), value: <span className="so-mono">{config.base_url}</span> },
                { label: t("app.model.model"), value: <span className="so-mono">{config.model}</span> },
                { label: t("app.model.key"), value: <Badge tone="success">{t("app.model.key.saved")}</Badge> },
              ]} /> : <p className="so-muted">{t("app.model.none")}</p>}
            </Card>
          )}
      </Section>
      {editing && <ModelDialog config={config} onClose={() => setEditing(false)} />}
    </div>
  );
}
