import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, Dialog, EmptyState, IconButton, Notice, PageHeader, Section, StatusDot } from "@open-ontology/design-system";
import { Cloud, Database, FolderOpen, HardDrive, PlugZap, Trash2 } from "lucide-react";
import { useSuperDriveActions, useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDrives } from "../model/superDriveSelectors";
import type { Drive } from "../../shared/super-ontology/docs-api.generated";
import { AddSourceMenu } from "./sources/AddSource";
import { useApp } from "../hooks/useApp";
import { invoke } from "../model/bridgeInvoke";
import { SourceDetail } from "./sources/SourceDetail";
import { useLoad } from "../hooks/useLoad";
import { useErrorText } from "../hooks/useText";
import { databaseApi } from "../model/databaseApi";
import { toSuperDriveError, type SuperDriveError } from "../model/superDriveErrors";
import type { DbSource } from "../../shared/super-ontology/bridge";

export function driveKindLabelId(drive: Drive): string {
  if (drive.backend !== "fs") return "app.sources.kind.cloud";
  return drive.name === "My files" ? "app.sources.kind.managed" : "app.sources.kind.folder";
}

/** The databases of the stack: name, where it lives, which tables are read; test the connection or remove it (its password file goes with it). */
function Databases({ items, onChanged }: { items: DbSource[]; onChanged: () => void }) {
  const { t } = useLocalization();
  const errorText = useErrorText();
  const [testing, setTesting] = useState<string | null>(null);
  const [outcome, setOutcome] = useState<{ name: string; reachable: boolean } | null>(null);
  const [removing, setRemoving] = useState<DbSource | null>(null);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const test = async (name: string) => {
    setTesting(name); setOutcome(null); setError(null);
    const result = await databaseApi.test(name);
    setTesting(null);
    if (result.ok) setOutcome({ name, reachable: result.value.reachable }); else setError(toSuperDriveError(result));
  };
  const remove = async (source: DbSource) => {
    const result = await databaseApi.remove(source.name);
    if (!result.ok) { setError(toSuperDriveError(result)); setRemoving(null); return; }
    setRemoving(null); setError(null); onChanged();
  };
  return (
    <Section title={t("app.sources.databases.title")}>
      {error && <div style={{ marginBottom: "var(--so-space-3)" }}><Notice tone="danger" title={t("app.sources.db.failed")}>{errorText(error)}</Notice></div>}
      <Card className="so-list" data-testid="db-source-list">
        {items.map((source) => (
          <div key={source.name} className="so-list-row" data-testid="db-source-row" data-name={source.name} data-kind={source.kind}>
            <span className="so-list-lead"><Database size={16} strokeWidth={1.5} aria-hidden="true" /></span>
            <span className="so-list-text">
              <span className="so-list-title so-truncate">{source.name}</span>
              <span className="so-list-sub so-truncate" title={source.include.join(", ")}>{source.kind === "mysql" ? "MySQL" : "PostgreSQL"} · {source.host}:{source.port}/{source.database} · {source.include.join(", ")}</span>
            </span>
            {outcome?.name === source.name && <Badge tone={outcome.reachable ? "success" : "danger"}>{t(outcome.reachable ? "app.sources.db.test.ok" : "app.sources.db.test.fail")}</Badge>}
            <IconButton icon={PlugZap} label={t("app.sources.db.test")} loading={testing === source.name} onClick={() => { void test(source.name); }} data-testid="db-source-test" />
            <IconButton icon={Trash2} label={t("app.sources.db.remove")} onClick={() => setRemoving(source)} data-testid="db-source-remove" />
          </div>
        ))}
      </Card>
      {removing && (
        <Dialog size="sm" title={t("app.sources.db.remove.title", { name: removing.name })} description={t("app.sources.db.remove.body")} onClose={() => setRemoving(null)} closeLabel={t("app.common.close")}
          footer={<><Button variant="ghost" onClick={() => setRemoving(null)}>{t("app.common.cancel")}</Button><Button variant="danger" icon={Trash2} onClick={() => { void remove(removing); }} data-testid="db-source-remove-confirm">{t("app.sources.db.remove")}</Button></>} />
      )}
    </Section>
  );
}

export function Sources() {
  const { t } = useLocalization();
  const actions = useSuperDriveActions();
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const app = useApp();
  const [detailId, setDetailId] = useState<string | null>(null);
  const [initial, setInitial] = useState<{ fileId: string; path: string } | null>(null);
  useEffect(() => { setDetailId(null); setInitial(null); }, [app.navTick]);
  const focus = app.focusFile;
  const { consumeFocusFile } = app;
  useEffect(() => {
    if (!focus) return undefined;
    let live = true;
    void invoke("file-detail", { fileId: focus }).then((result) => {
      if (!live) return;
      consumeFocusFile();
      if (result.ok) { actions.openDrive(result.value.drive_id); setInitial({ fileId: focus, path: result.value.path }); setDetailId(result.value.drive_id); }
    });
    return () => { live = false; };
  }, [focus, actions, consumeFocusFile]);
  const databases = useLoad(() => databaseApi.list(), [], {});
  const detail = drives.find((d) => d.id === detailId) ?? null;
  const open = (id: string) => { actions.openDrive(id); setDetailId(id); };

  if (detail) return <SourceDetail key={detail.id} drive={detail} initial={initial} onBack={() => { setDetailId(null); setInitial(null); }} />;
  return (
    <div className="so-page" data-testid="screen-sources">
      <PageHeader title={t("app.sources.title")} description={t("app.sources.description")} actions={<AddSourceMenu onOpened={open} onDatabaseAdded={databases.reload} />} />
      {drives.length === 0 && (databases.value?.items.length ?? 0) === 0 && databases.status !== "loading" ? (
        <Card><EmptyState icon={FolderOpen} title={t("app.sources.empty.title")} description={t("app.sources.empty.body")} action={<AddSourceMenu onOpened={open} onDatabaseAdded={databases.reload} />} /></Card>
      ) : drives.length > 0 && (
        <Card className="so-list" data-testid="source-list">
          {drives.map((drive) => {
            const fresh = drive.freshness?.state;
            const Glyph = drive.backend !== "fs" ? Cloud : drive.name === "My files" ? HardDrive : FolderOpen;
            return (
              <button key={drive.id} type="button" className="so-list-row" onClick={() => open(drive.id)} data-testid="source-row" data-name={drive.name} data-kind={drive.backend !== "fs" ? "cloud" : drive.name === "My files" ? "managed" : "folder"} data-state={fresh === "syncing" || fresh === "processing" ? "syncing" : fresh === "degraded" || fresh === "held" ? "error" : "idle"}>
                <span className="so-list-lead"><Glyph size={16} strokeWidth={1.5} aria-hidden="true" /></span>
                <span className="so-list-text">
                  <span className="so-list-title so-truncate">{drive.name}</span>
                  <span className="so-list-sub so-truncate">{t(driveKindLabelId(drive))} · {t("app.sources.files", { count: drive.files_total ?? 0 })}</span>
                </span>
                {drive.status === "paused" ? <StatusDot tone="warning" label={t("app.sources.paused")} /> : <StatusDot tone={fresh === "up_to_date" ? "success" : fresh === "degraded" || fresh === "held" ? "warning" : fresh ? "accent" : "neutral"} label={fresh ? t(`app.freshness.${fresh}`) : t("app.home.sources.unknown")} pulse={fresh === "processing" || fresh === "syncing"} />}
                {(drive.pending_review ?? 0) > 0 && <span className="so-badge" data-tone="warning">{t("app.sources.review", { count: drive.pending_review ?? 0 })}</span>}
              </button>
            );
          })}
        </Card>
      )}
      {databases.value && databases.value.items.length > 0 && <div style={{ marginTop: "var(--so-space-6)" }}><Databases items={databases.value.items} onChanged={databases.reload} /></div>}
      <p className="so-weak" style={{ marginTop: "var(--so-space-4)" }}>{t("super-drive.common.dataLocal")}</p>
    </div>
  );
}
