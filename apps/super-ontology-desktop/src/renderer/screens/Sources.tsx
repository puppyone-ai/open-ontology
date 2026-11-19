import { useEffect, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Card, EmptyState, PageHeader, StatusDot } from "@open-ontology/design-system";
import { Cloud, FolderOpen, HardDrive } from "lucide-react";
import { useSuperDriveActions, useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDrives } from "../model/superDriveSelectors";
import type { Drive } from "../../shared/super-ontology/docs-api.generated";
import { AddSourceMenu } from "./sources/AddSource";
import { useApp } from "../hooks/useApp";
import { invoke } from "../model/bridgeInvoke";
import { SourceDetail } from "./sources/SourceDetail";

export function driveKindLabelId(drive: Drive): string {
  if (drive.backend !== "fs") return "app.sources.kind.cloud";
  return drive.name === "My files" ? "app.sources.kind.managed" : "app.sources.kind.folder";
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
  const detail = drives.find((d) => d.id === detailId) ?? null;
  const open = (id: string) => { actions.openDrive(id); setDetailId(id); };

  if (detail) return <SourceDetail key={detail.id} drive={detail} initial={initial} onBack={() => { setDetailId(null); setInitial(null); }} />;
  return (
    <div className="so-page" data-testid="screen-sources">
      <PageHeader title={t("app.sources.title")} description={t("app.sources.description")} actions={<AddSourceMenu onOpened={open} />} />
      {drives.length === 0 ? (
        <Card><EmptyState icon={FolderOpen} title={t("app.sources.empty.title")} description={t("app.sources.empty.body")} action={<AddSourceMenu onOpened={open} />} /></Card>
      ) : (
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
      <p className="so-weak" style={{ marginTop: "var(--so-space-4)" }}>{t("super-drive.common.dataLocal")}</p>
    </div>
  );
}
