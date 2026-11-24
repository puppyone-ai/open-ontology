import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, EmptyState, Notice, Skeleton } from "@open-ontology/design-system";
import { ScrollText, ShieldCheck } from "lucide-react";
import { useLoad } from "../../hooks/useLoad";
import { useRelativeTime } from "../../hooks/useText";
import { ErrorText } from "../shared/ErrorText";
import { superDriveApi } from "../../model/superDriveApi";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { isEngineReady } from "../../model/superDriveStateModel";
import type { AuditVerifyResult } from "../../../shared/super-ontology/bridge";
import type { SuperDriveError } from "../../model/superDriveErrors";

/** Who was given what, and when: an append-only record. It can be checked, never edited. */
export function AccessLog() {
  const { t } = useLocalization();
  const relative = useRelativeTime();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const log = useLoad(() => superDriveApi.auditShares({}), [ready], { enabled: ready });
  const [verify, setVerify] = useState<{ status: "idle" | "checking" } | { status: "done"; result: AuditVerifyResult } | { status: "error"; error: SuperDriveError }>({ status: "idle" });
  const check = async () => {
    setVerify({ status: "checking" });
    const result = await superDriveApi.auditVerify();
    setVerify(result.ok ? { status: "done", result: result.value } : { status: "error", error: result.error });
  };
  const items = [...(log.value?.items ?? [])].sort((a, b) => b.seq - a.seq);
  return (
    <div className="so-stack" data-testid="access-log">
      <div className="so-row"><p className="so-muted so-grow">{t("app.access.intro")}</p><Button icon={ShieldCheck} loading={verify.status === "checking"} onClick={() => { void check(); }}>{t("app.access.verify")}</Button></div>
      {verify.status === "done" && <Notice tone={verify.result.intact ? "success" : "danger"} title={verify.result.intact ? t("app.access.intact") : t("app.access.broken")}>{verify.result.intact ? t("app.access.intact.body", { count: verify.result.rows ?? 0 }) : t("app.access.broken.body", { seq: verify.result.broken_at_seq ?? 0 })}</Notice>}
      {verify.status === "error" && <Notice tone="danger" title={t("app.common.loadFailed")}><ErrorText error={verify.error} /></Notice>}
      {log.status === "error" && !log.value ? <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={log.reload}>{t("app.common.retry")}</Button>}><ErrorText error={log.error} /></Notice>
        : log.status === "loading" && !log.value ? <Card className="so-list" aria-busy="true">{Array.from({ length: 3 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="50%" height={13} /></div>)}</Card>
        : items.length === 0 ? <Card><EmptyState icon={ScrollText} title={t("app.access.empty.title")} description={t("app.access.empty.body")} /></Card>
        : (
          <Card className="so-list">
            {items.map((item) => (
              <div key={item.seq} className="so-list-row">
                <span className="so-list-text"><span className="so-list-title so-truncate">{item.agent || item.actor}</span><span className="so-list-sub so-truncate">{[item.role, relative(item.at), t("app.access.reads", { count: item.refs_reads })].filter(Boolean).join(" · ")}</span></span>
                {item.revoked_at ? <Badge>{t("app.access.revoked")}</Badge> : null}
              </div>
            ))}
          </Card>
        )}
    </div>
  );
}
