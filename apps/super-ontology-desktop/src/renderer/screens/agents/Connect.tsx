import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, CopyBlock, EmptyState, Icon, Notice, Section, Segmented, Select, Skeleton, TextField, Badge, Dialog } from "@open-ontology/design-system";
import { KeyRound, Lock, Trash2 } from "lucide-react";
import { useLoad } from "../../hooks/useLoad";
import { useErrorText, useRelativeTime } from "../../hooks/useText";
import { agentApi } from "../../model/agentApi";
import { AGENT_KINDS, loadSnippet, type AgentKind } from "../../model/agentConfigs";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives } from "../../model/superDriveSelectors";
import { isEngineReady } from "../../model/superDriveStateModel";
import type { AgentConnectSnippet, AgentTokenIssued } from "../../../shared/super-ontology/bridge";
import type { SuperDriveError } from "../../model/superDriveErrors";

const NAME = /^[A-Za-z0-9._-]{1,64}$/u;
const TTL_DAYS = [1, 7, 30, 90] as const;

/** Step by step: name an Agent, issue a read-only token, copy the connection text of the Agent it will run in. */
export function Connect({ compact = false }: { compact?: boolean } = {}) {
  const { t } = useLocalization();
  const errorText = useErrorText();
  const relative = useRelativeTime();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const tokens = useLoad(() => agentApi.list(), [ready], { enabled: ready });
  const [name, setName] = useState("my-agent");
  const [driveId, setDriveId] = useState("all");
  const [ttl, setTtl] = useState<number>(7);
  const [kind, setKind] = useState<AgentKind>("generic");
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const [issued, setIssued] = useState<AgentTokenIssued | null>(null);
  const [snippet, setSnippet] = useState<AgentConnectSnippet | null>(null);
  const [revoking, setRevoking] = useState<{ id: string; name: string } | null>(null);

  const taken = (tokens.value?.items ?? []).some((token) => token.name === name && !token.revoked_at && token.id !== issued?.id);
  const nameError = touched && !NAME.test(name) ? t("app.connect.name.bad") : taken || (error?.code === "DOCS_ENGINE_BAD_REQUEST" && error.details.pointer === "/name") ? t("app.connect.name.taken") : null;
  const create = async () => {
    setTouched(true);
    if (!NAME.test(name) || taken) return;
    setBusy(true); setError(null);
    const result = await agentApi.create({ name, scopes: ["docs:read"], ...(driveId === "all" ? { allDrives: true } : { driveIds: [driveId] }), expiresInSeconds: ttl * 86_400 });
    setBusy(false);
    if (!result.ok) { setError(result.error); return; }
    setIssued(result.value); tokens.reload();
  };
  const revoke = async () => {
    if (!revoking) return;
    const result = await agentApi.revoke(revoking.id);
    setRevoking(null);
    if (!result.ok) setError(result.error); else tokens.reload();
  };
  const issuedName = issued?.name ?? null;
  useEffect(() => {
    if (!issuedName) { setSnippet(null); return undefined; }
    let live = true;
    void loadSnippet(kind, issuedName).then((result) => { if (live && result.ok) setSnippet(result.value); });
    return () => { live = false; };
  }, [issuedName, kind]);

  if (!ready) return <Notice tone="warning" title={t("app.connect.engine.title")}>{t("app.connect.engine.body")}</Notice>;
  const locked = !issued;
  return (
    <div className="so-stack" data-testid="connect">
      <Section title={t("app.connect.step1.title")} description={t("app.connect.step1.body")}>
        <Card className="so-form" data-testid={compact ? "firstrun-token-create" : "agent-token-create"}>
          {error && !(error.code === "DOCS_ENGINE_BAD_REQUEST" && error.details.pointer === "/name") && <Notice tone="danger" title={t("app.connect.failed")}>{errorText(error)}</Notice>}
          <div className="so-grid" data-cols="2">
            <TextField data-testid="agent-token-name" label={t("app.connect.name")} value={name} onChange={(e) => setName(e.target.value)} error={nameError} helper={t("app.connect.name.help")} maxLength={64} spellCheck={false} />
            <Select label={t("app.connect.drives")} value={driveId} onChange={(e) => setDriveId(e.target.value)} helper={t("app.connect.drives.help")}>
              <option value="all">{t("app.connect.drives.all")}</option>
              {drives.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}
            </Select>
            <Select label={t("app.connect.ttl")} value={String(ttl)} onChange={(e) => setTtl(Number(e.target.value))} helper={t("app.connect.ttl.help")}>
              {TTL_DAYS.map((d) => <option key={d} value={d}>{t("app.connect.ttl.days", { count: d })}</option>)}
            </Select>
            <div className="so-field"><span className="so-field-label">{t("app.connect.access")}</span><span className="so-row" style={{ height: "var(--so-control)" }}><Badge tone="accent">{t("app.connect.readonly")}</Badge><span className="so-weak so-truncate">{t("app.connect.readonly.help")}</span></span></div>
          </div>
          <div className="so-row">
            <Button variant="primary" icon={KeyRound} loading={busy} onClick={() => { void create(); }} disabled={drives.length === 0} data-testid={compact ? "firstrun-create-token" : "agent-token-submit"}>{t("app.connect.create")}</Button>
            {drives.length === 0 && <span className="so-weak">{t("app.connect.needsSource")}</span>}
          </div>
        </Card>
      </Section>

      <Section title={t("app.connect.step2.title")} description={locked ? t("app.connect.step2.locked") : t("app.connect.step2.body")}>
        <Card className="so-form so-step" data-state={locked ? "locked" : "ready"} aria-disabled={locked || undefined} data-testid="connect-config">
          {locked ? (
            <div className="so-row so-step-locked"><Icon icon={Lock} size={16} /><span>{t("app.connect.step2.locked.hint")}</span></div>
          ) : (
            <>
              <Notice tone="warning" title={t("app.connect.once.title")}>{t("app.connect.once.body")}</Notice>
              <CopyBlock label={t("app.connect.token")} value={issued.token} copyLabel={t("app.common.copy")} copiedLabel={t("app.common.copied")} valueTestId="agent-token-secret" />
              <div className="so-stack">
                <Segmented<AgentKind> label={t("app.connect.agent")} value={kind} onChange={setKind} options={AGENT_KINDS.map((value) => ({ value, label: t(`app.connect.agent.${value}`), testId: compact ? `firstrun-agent-${value}` : `agent-kind-${value}` }))} />
                <p className="so-muted">{t(`app.connect.where.${kind}`)}</p>
                {snippet ? <CopyBlock label={t("app.connect.config")} value={snippet.text} copyLabel={t("app.common.copy")} copiedLabel={t("app.common.copied")} valueTestId={compact ? "firstrun-config-text" : "agent-config-text"} copyTestId={compact ? "firstrun-config-copy" : "agent-config-copy"} /> : <Skeleton height={96} radius={8} />}
                {snippet && <CopyBlock label={t("app.connect.cli.title")} value={snippet.cli} copyLabel={t("app.common.copy")} copiedLabel={t("app.common.copied")} valueTestId={compact ? "firstrun-cli-line" : "agent-cli-line"} multiline />}
              </div>
            </>
          )}
        </Card>
      </Section>

      {!compact && <Section title={t("app.connect.tokens.title")}>
        {tokens.status === "loading" && !tokens.value ? <Card className="so-list" aria-busy="true">{Array.from({ length: 2 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="40%" height={13} /></div>)}</Card>
          : tokens.status === "error" && !tokens.value ? <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={tokens.reload}>{t("app.common.retry")}</Button>}>{errorText(tokens.error)}</Notice>
          : (tokens.value?.items ?? []).length === 0 ? <Card><EmptyState icon={KeyRound} title={t("app.connect.tokens.empty.title")} description={t("app.connect.tokens.empty.body")} compact /></Card>
          : (
            <Card className="so-list" data-testid="agent-token-list">
              {(tokens.value?.items ?? []).map((token) => (
                <div key={token.id} className="so-list-row" data-testid="agent-token-row" data-name={token.name} data-state={token.revoked_at ? "revoked" : "active"}>
                  <span className="so-list-text"><span className="so-list-title so-truncate">{token.name}</span><span className="so-list-sub so-truncate">{[token.scopes.join(", "), token.expires_at ? t("app.connect.tokens.expires", { when: relative(token.expires_at) }) : null, token.last_used_at ? t("app.connect.tokens.used", { when: relative(token.last_used_at) }) : t("app.connect.tokens.neverUsed")].filter(Boolean).join(" · ")}</span></span>
                  {token.revoked_at ? <Badge>{t("app.connect.tokens.revoked")}</Badge> : <Button size="compact" variant="quiet-danger" icon={Trash2} onClick={() => setRevoking({ id: token.id, name: token.name })} data-testid="agent-token-revoke">{t("app.connect.tokens.revoke")}</Button>}
                </div>
              ))}
            </Card>
          )}
      </Section>}

      {revoking && (
        <Dialog size="sm" testId="dialog" title={t("app.connect.revoke.title", { name: revoking.name })} description={t("app.connect.revoke.body")} onClose={() => setRevoking(null)} closeLabel={t("app.common.close")}
          footer={<><Button variant="ghost" onClick={() => setRevoking(null)} data-testid="dialog-cancel">{t("app.common.cancel")}</Button><Button variant="danger" icon={Trash2} onClick={() => { void revoke(); }} data-testid="dialog-confirm">{t("app.connect.tokens.revoke")}</Button></>} />
      )}
    </div>
  );
}
