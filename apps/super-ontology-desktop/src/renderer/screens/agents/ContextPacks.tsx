import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, Dialog, EmptyState, Notice, Select, Skeleton, StatusDot, TextArea, type Tone } from "@open-ontology/design-system";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Eye, FolderOpen, PackagePlus, RefreshCw, Trash2 } from "lucide-react";
import { useRelativeTime } from "../../hooks/useText";
import { ErrorText } from "../shared/ErrorText";
import type { Share } from "../../../shared/super-ontology/docs-api.generated";
import { createOperationId } from "../../model/operationIds";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives } from "../../model/superDriveSelectors";
import type { SuperDriveError } from "../../model/superDriveErrors";

const STATE_TONE: Record<string, Tone> = { active: "success", drifted: "warning", refreshed: "neutral", expired: "neutral", revoked: "neutral" };

function PackRow({ share, driveName, onOpen }: { share: Share; driveName: string; onOpen: (share: Share) => void }) {
  const { t } = useLocalization();
  const relative = useRelativeTime();
  const actions = useSuperDriveActions();
  const pending = useSuperDriveSelector((s) => s.shares.pending[share.id] ?? null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const live = share.state === "active" || share.state === "drifted";
  const run = async (op: "refresh" | "revoke") => {
    setError(null);
    const result = op === "refresh" ? await actions.refreshShare(share.id) : await actions.revokeShare(share.id);
    if (!result.ok) setError(result.error);
  };
  return (
    <div className="so-list-row" data-testid="pack-row" style={{ flexWrap: "wrap" }}>
      <span className="so-list-text">
        <span className="so-list-title so-truncate">{share.task?.trim() || t("app.packs.untitled")}</span>
        <span className="so-list-sub so-truncate">{[driveName, t(`super-drive.share.role.${share.role}`), t("app.packs.expires", { when: relative(share.expires_at) })].join(" · ")}</span>
        {error && <span role="alert" className="so-list-sub" style={{ color: "var(--so-danger)" }}><ErrorText error={error} /></span>}
      </span>
      <StatusDot tone={STATE_TONE[share.state] ?? "neutral"} label={share.state === "drifted" ? t("app.packs.state.drifted") : t(`app.packs.state.${share.state}`)} />
      <span className="so-row">
        <Button size="compact" variant="ghost" icon={Eye} onClick={() => onOpen(share)}>{t("app.packs.brief")}</Button>
        {share.state === "drifted" && <Button size="compact" icon={RefreshCw} loading={pending === "refreshing"} onClick={() => { void run("refresh"); }}>{t("app.packs.refresh")}</Button>}
        {live && <Button size="compact" variant="danger" icon={Trash2} loading={pending === "revoking"} onClick={() => { void run("revoke"); }}>{t("app.packs.revoke")}</Button>}
      </span>
    </div>
  );
}

function BriefDialog({ share, onClose }: { share: Share; onClose: () => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const [brief, setBrief] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  useEffect(() => {
    let live = true;
    void actions.readPack(share.pack_id).then((result) => { if (!live) return; if (result.ok) setBrief(result.value.brief_preview ?? ""); else setFailed(true); });
    return () => { live = false; };
  }, [actions, share.pack_id]);
  return (
    <Dialog size="lg" title={t("app.packs.brief.title")} description={share.task ?? undefined} onClose={onClose} closeLabel={t("app.common.close")} footer={<Button onClick={onClose}>{t("app.common.close")}</Button>}>
      {failed ? <Notice tone="danger" title={t("app.common.loadFailed")} /> : brief === null ? <div className="so-stack" aria-busy="true"><Skeleton height={14} /><Skeleton height={14} width="80%" /><Skeleton height={14} width="65%" /></div>
        : brief === "" ? <p className="so-muted">{t("app.packs.brief.empty")}</p> : <div className="so-brief"><Markdown remarkPlugins={[remarkGfm]} skipHtml>{brief}</Markdown></div>}
    </Dialog>
  );
}

function CreateDialog({ onClose }: { onClose: () => void }) {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const [driveId, setDriveId] = useState(drives[0]?.id ?? "");
  const [task, setTask] = useState("");
  const [target, setTarget] = useState<{ grantId: string; displayPath: string } | null>(null);
  const [touched, setTouched] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const pick = async () => {
    const picked = await actions.pickMountTarget();
    if (!picked.ok) { setError(picked.error); return; }
    if (!("cancelled" in picked.value)) { setTarget(picked.value); setError(null); }
  };
  const create = async () => {
    setTouched(true);
    if (!driveId || !target) return;
    setBusy(true); setError(null);
    const result = await actions.mountShare({ compile: { driveId, ...(task.trim() ? { task: task.trim() } : {}), budget: 800, role: "viewer" }, targetGrantId: target.grantId, expiresInSeconds: 7 * 86_400, operationId: createOperationId("sh") });
    setBusy(false);
    if (!result.ok) { setError(result.error); return; }
    onClose();
  };
  return (
    <Dialog title={t("app.packs.create.title")} description={t("app.packs.create.description")} onClose={busy ? undefined : onClose} closeLabel={t("app.common.close")}
      footer={<><Button variant="ghost" onClick={onClose} disabled={busy}>{t("app.common.cancel")}</Button><Button variant="primary" loading={busy} icon={PackagePlus} onClick={() => { void create(); }}>{t("app.packs.create.confirm")}</Button></>}>
      {error && <Notice tone="danger" title={t("app.packs.create.failed")}><ErrorText error={error} /></Notice>}
      <Select label={t("app.packs.create.source")} value={driveId} onChange={(e) => setDriveId(e.target.value)}>{drives.map((d) => <option key={d.id} value={d.id}>{d.name}</option>)}</Select>
      <TextArea label={t("app.packs.create.task")} value={task} onChange={(e) => setTask(e.target.value)} placeholder={t("app.packs.create.task.placeholder")} helper={t("app.packs.create.task.help")} optionalLabel={t("app.sources.cloud.optional")} maxLength={2000} />
      <div className="so-field" data-invalid={touched && !target ? "true" : undefined}>
        <span className="so-field-label">{t("app.packs.create.folder")}</span>
        <div className="so-row"><Button icon={FolderOpen} onClick={() => { void pick(); }}>{t("app.packs.create.pick")}</Button><span className="so-mono so-truncate so-grow so-muted">{target?.displayPath ?? ""}</span></div>
        {touched && !target ? <p className="so-field-error" role="alert">{t("app.packs.create.folder.required")}</p> : <p className="so-field-helper">{t("app.packs.create.folder.help")}</p>}
      </div>
    </Dialog>
  );
}

/** A context pack is what an Agent gets to read: a BRIEF with its sources, kept current, revocable. */
export function ContextPacks() {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const shares = useSuperDriveSelector((s) => s.shares);
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const [brief, setBrief] = useState<Share | null>(null);
  const [creating, setCreating] = useState(false);
  useEffect(() => { void actions.loadShares({}); }, [actions]);
  const names = new Map(drives.map((d) => [d.id, d.name]));
  // A refreshed pack is replaced by its newer one: only the newest of every chain is listed.
  const superseded = new Set(Object.values(shares.chains).flatMap((chain) => chain.slice(0, -1)));
  const visible = shares.order.map((id) => shares.byId[id]).filter((s): s is Share => Boolean(s) && !superseded.has(s.id));
  return (
    <div className="so-stack" data-testid="packs">
      <div className="so-row"><p className="so-muted so-grow">{t("app.packs.intro")}</p><Button variant="primary" icon={PackagePlus} disabled={drives.length === 0} onClick={() => setCreating(true)} data-testid="create-pack">{t("app.packs.create")}</Button></div>
      {shares.error && !shares.loaded ? <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={() => { void actions.loadShares({ force: true }); }}>{t("app.common.retry")}</Button>}><ErrorText error={shares.error} /></Notice>
        : !shares.loaded ? <Card className="so-list" aria-busy="true">{Array.from({ length: 2 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="45%" height={13} /></div>)}</Card>
        : visible.length === 0 ? <Card><EmptyState icon={PackagePlus} title={t("app.packs.empty.title")} description={t("app.packs.empty.body")} /></Card>
        : <Card className="so-list">{visible.map((share) => <PackRow key={share.id} share={share} driveName={names.get(share.drive_id) ?? ""} onOpen={setBrief} />)}</Card>}
      {brief && <BriefDialog share={brief} onClose={() => setBrief(null)} />}
      {creating && <CreateDialog onClose={() => setCreating(false)} />}
    </div>
  );
}
