import { useMemo, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, Dialog, EmptyState, IconButton, Menu, MenuItem, Notice, PageHeader, ProgressBar, Segmented, Skeleton, StatusDot, Tooltip } from "@open-ontology/design-system";
import { ArrowLeft, ChevronRight, File as FileIcon, FileUp, Folder, MoreHorizontal, RefreshCw, Search, Trash2 } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { useDriveFolderPages } from "../../model/browser/useDriveFolderPages";
import type { Drive } from "../../../shared/super-ontology/docs-api.generated";
import { formatBytes } from "../../model/format";
import { getUploadQueue, useUploadQueueState } from "../../model/uploadStore";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import type { SuperDriveError } from "../../model/superDriveErrors";
import { isUploadActive } from "../../model/queue/uploadQueue";
import { statesOfGroups, type FileFilterGroup } from "../../model/superDriveStateModel";
import { driveKindLabelId } from "../Sources";
import { FilePanel } from "./FilePanel";
import { fileView, TONE, useFileStateText } from "./fileView";

type Filter = "all" | "processing" | "ready" | "attention";
const GROUPS: Record<Exclude<Filter, "all">, FileFilterGroup[]> = { processing: ["processing"], ready: ["published"], attention: ["failed", "skipped"] };

export function SourceDetail({ drive, initial = null, onBack }: { drive: Drive; initial?: { fileId: string; path: string } | null; onBack: () => void }) {
  const t$ = useLocalization();
  const { t, formatNumber, formatDate } = t$;
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const stateText = useFileStateText();
  const [path, setPath] = useState(() => (initial ? initial.path.split("/").slice(0, -1).join("/") : ""));
  const [filter, setFilter] = useState<Filter>("all");
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState<string | null>(initial?.fileId ?? null);
  const [syncing, setSyncing] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const [removing, setRemoving] = useState(false);
  const [dragging, setDragging] = useState(false);
  const menuAnchor = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState(false);
  const uploads = useUploadQueueState();
  const active = uploads.items.filter((item) => item.driveId === drive.id && isUploadActive(item));
  const readOnly = drive.backend !== "fs";

  const folderFilter = useMemo(() => ({ ...(filter === "all" ? {} : { states: statesOfGroups(GROUPS[filter]) }), ...(query.trim() ? { q: query.trim(), recursive: true } : {}) }), [filter, query]);
  const { page, files, initialLoading, loadMore, reload } = useDriveFolderPages(drive.id, path, folderFilter);
  const failed = useSuperDriveSelector((s) => s.drives.byId[drive.id]?.counts?.parse_failed ?? 0) as number;
  const segments = path ? path.split("/") : [];
  const fresh = drive.freshness?.state;

  const sync = async () => {
    setSyncing(true); setError(null);
    const result = await actions.syncDrive(drive.id);
    setSyncing(false);
    if (!result.ok) setError(result.error);
  };
  const remove = async () => {
    const result = await actions.deleteDrive(drive.id);
    if (!result.ok) { setError(result.error); setRemoving(false); return; }
    onBack();
  };
  const onDrop = (event: React.DragEvent) => {
    event.preventDefault(); setDragging(false);
    if (readOnly) return;
    void getUploadQueue().enqueueFiles(drive.id, path, event.dataTransfer.files);
  };
  const pickFiles = useRef<HTMLInputElement>(null);

  return (
    <div className="so-page" data-wide={selected ? "" : undefined} data-testid="screen-source-detail" data-name={drive.name}
      onDragOver={(event) => { if (!readOnly) { event.preventDefault(); setDragging(true); } }} onDragLeave={(event) => { if (event.currentTarget === event.target) setDragging(false); }} onDrop={onDrop}>
      <div style={{ margin: "calc(-1 * var(--so-space-2)) 0 var(--so-space-2)" }}>
        <Button variant="ghost" size="compact" icon={ArrowLeft} onClick={onBack}>{t("app.sources.title")}</Button>
      </div>
      <PageHeader title={drive.name} description={[drive.name === "My files" ? null : t(driveKindLabelId(drive)), t("app.sources.files", { count: drive.files_total ?? 0 }), drive.bytes_total ? formatBytes(drive.bytes_total, t$) : null].filter(Boolean).join(" · ")}
        actions={<>
          {fresh && <span style={{ display: "contents" }} data-testid="source-freshness" data-freshness={fresh}><StatusDot tone={fresh === "up_to_date" ? "success" : fresh === "degraded" || fresh === "held" ? "warning" : "accent"} label={t(`app.freshness.${fresh}`)} pulse={fresh === "processing" || fresh === "syncing"} /></span>}
          <Button icon={RefreshCw} loading={syncing} onClick={() => { void sync(); }} data-testid="source-sync">{t("app.sources.sync")}</Button>
          {!readOnly && <><input ref={pickFiles} type="file" multiple hidden onChange={(e) => { if (e.target.files) void getUploadQueue().enqueueFiles(drive.id, path, e.target.files); e.target.value = ""; }} /><Button variant="primary" icon={FileUp} onClick={() => pickFiles.current?.click()}>{t("app.sources.upload")}</Button></>}
          <IconButton ref={menuAnchor} icon={MoreHorizontal} label={t("app.sources.more")} variant="secondary" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((v) => !v)} />
          <Menu anchor={menuAnchor} open={menu} onClose={() => setMenu(false)} ariaLabel={t("app.sources.more")} placement="bottom-end">
            <MenuItem icon={Trash2} tone="danger" label={t("app.sources.remove")} closeMenu={() => setMenu(false)} onSelect={() => setRemoving(true)} />
          </Menu>
        </>} />

      {error && <div style={{ marginBottom: "var(--so-space-4)" }}><Notice tone="danger" title={t("app.sources.actionFailed")} action={<Button size="compact" onClick={() => setError(null)}>{t("app.common.close")}</Button>}>{errorText(error)}</Notice></div>}
      {Number(failed) > 0 && filter !== "attention" && <div style={{ marginBottom: "var(--so-space-4)" }}><Notice tone="warning" title={t("app.sources.failed", { count: Number(failed) })} action={<Button size="compact" onClick={() => setFilter("attention")}>{t("app.sources.failed.show")}</Button>}>{t("app.sources.failed.body")}</Notice></div>}
      {active.length > 0 && (
        <Card className="so-metric" role="status" aria-live="polite" style={{ marginBottom: "var(--so-space-4)" }}>
          <div className="so-row"><span className="so-list-title">{t("app.sources.uploading", { count: active.length })}</span></div>
          <ProgressBar label={t("app.sources.uploading", { count: active.length })} value={(() => { const total = active.reduce((n, i) => n + i.size, 0); return total > 0 ? active.reduce((n, i) => n + i.bytesDone, 0) / total : null; })()} />
        </Card>
      )}

      <div className="so-toolbar" style={{ marginBottom: "var(--so-space-3)" }}>
        <div className="so-search">
          <Search size={16} strokeWidth={1.5} className="so-search-icon so-icon" aria-hidden="true" />
          <input className="so-input" type="search" value={query} onChange={(e) => setQuery(e.target.value)} placeholder={t("app.sources.search")} aria-label={t("app.sources.search")} />
        </div>
        <Segmented<Filter> label={t("app.sources.filter")} value={filter} onChange={setFilter} options={[{ value: "all", label: t("app.sources.filter.all") }, { value: "processing", label: t("app.sources.filter.processing") }, { value: "ready", label: t("app.sources.filter.ready") }, { value: "attention", label: t("app.sources.filter.attention") }]} />
      </div>
      <nav className="so-row" aria-label={t("app.sources.path")} style={{ marginBottom: "var(--so-space-2)", minHeight: 28 }}>
        <Button variant="ghost" size="compact" onClick={() => setPath("")} aria-current={path === "" ? "location" : undefined}>{drive.name}</Button>
        {segments.map((segment, index) => (
          <span key={segments.slice(0, index + 1).join("/")} className="so-row">
            <ChevronRight size={16} strokeWidth={1.5} className="so-weak so-icon" aria-hidden="true" />
            <Button variant="ghost" size="compact" onClick={() => setPath(segments.slice(0, index + 1).join("/"))}>{segment}</Button>
          </span>
        ))}
      </nav>

      <div className={selected ? "so-split" : undefined}>
        <Card className="so-dropzone-host" data-testid="file-panel" data-dragging={dragging ? "true" : undefined} style={{ overflow: "hidden", outline: dragging ? "2px solid var(--so-accent)" : undefined }}>
          {initialLoading ? (
            <div className="so-list" aria-busy="true">{Array.from({ length: 5 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width={16} height={16} /><Skeleton width={`${40 + (i % 3) * 15}%`} height={13} /></div>)}</div>
          ) : page.error && files.length === 0 ? (
            <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={reload}>{t("app.common.retry")}</Button>}>{errorText(page.error)}</Notice>
          ) : files.length === 0 && page.dirs.length === 0 ? (
            <EmptyState icon={filter === "all" && !query ? FileUp : Search} title={filter === "all" && !query ? t("app.sources.files.empty.title") : t("app.sources.files.none.title")} description={filter === "all" && !query ? (readOnly ? t("app.sources.files.empty.readonly") : t("app.sources.files.empty.body")) : t("app.sources.files.none.body")}
              action={filter === "all" && !query && !readOnly ? <Button variant="primary" icon={FileUp} onClick={() => pickFiles.current?.click()}>{t("app.sources.upload")}</Button> : undefined} />
          ) : (
            <table className="so-table" style={{ tableLayout: "fixed" }}>
              <colgroup><col /><col style={{ width: 168 }} /><col style={{ width: 84 }} /><col style={{ width: 112 }} /></colgroup>
              <thead><tr><th>{t("app.sources.col.name")}</th><th>{t("app.sources.col.status")}</th><th data-numeric>{t("app.sources.col.size")}</th><th>{t("app.sources.col.modified")}</th></tr></thead>
              <tbody>
                {page.dirs.map((dir) => (
                  <tr key={`d:${dir.path}`} data-selectable onDoubleClick={() => setPath(dir.path)} onClick={() => setPath(dir.path)} style={{ cursor: "default" }}>
                    <td colSpan={4}><span className="so-row"><Folder size={16} strokeWidth={1.5} className="so-icon" aria-hidden="true" /><span className="so-truncate so-grow">{dir.path.slice(dir.path.lastIndexOf("/") + 1)}</span></span></td>
                  </tr>
                ))}
                {files.map((file) => {
                  const view = fileView(file);
                  const label = stateText(view);
                  return (
                    <tr key={file.id} data-selectable aria-selected={selected === file.id} tabIndex={0} onKeyDown={(e) => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); setSelected(file.id); } }} onClick={() => setSelected(file.id)} data-testid="file-row" data-name={file.name ?? file.path} data-state={file.state}>
                      <td><span className="so-row"><FileIcon size={16} strokeWidth={1.5} className="so-icon so-weak" aria-hidden="true" /><Tooltip label={file.path}><span className="so-truncate so-grow">{file.name ?? file.path}</span></Tooltip></span></td>
                      <td><span className="so-row"><StatusDot tone={TONE[view.tone]} label={label} pulse={view.busy} />{view.reviewCount > 0 && <Badge tone="warning">{formatNumber(view.reviewCount)}</Badge>}</span></td>
                      <td data-numeric className="so-muted">{formatBytes(file.size, t$)}</td>
                      <td className="so-muted so-truncate">{file.modified_at ? formatDate(new Date(file.modified_at), { dateStyle: "medium" }) : ""}</td>
                    </tr>
                  );
                })}
              </tbody>
            </table>
          )}
          {page.nextCursor && <div className="so-row" style={{ justifyContent: "center", padding: "var(--so-space-3)" }}><Button variant="ghost" loading={page.loading} onClick={loadMore}>{t("app.sources.more.load")}</Button></div>}
        </Card>
        {selected && <FilePanel fileId={selected} onClose={() => setSelected(null)} />}
      </div>

      {removing && (
        <Dialog size="sm" title={t("app.sources.remove.title", { name: drive.name })} description={readOnly ? t("app.sources.remove.body.cloud") : t("app.sources.remove.body.folder")} onClose={() => setRemoving(false)} closeLabel={t("app.common.close")}
          footer={<><Button variant="ghost" onClick={() => setRemoving(false)}>{t("app.common.cancel")}</Button><Button variant="danger" icon={Trash2} onClick={() => { void remove(); }}>{t("app.sources.remove.confirm")}</Button></>} />
      )}
    </div>
  );
}
