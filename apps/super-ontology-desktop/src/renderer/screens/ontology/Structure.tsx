import { useMemo, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Badge, Button, Card, Dialog, EmptyState, IconButton, Menu, MenuItem, Notice, ProgressBar, Select, Skeleton, TextField } from "@open-ontology/design-system";
import { Check, GitMerge, Layers, MoreHorizontal, Pencil, RefreshCw, X } from "lucide-react";
import type { StructureClass, StructureProperty, StructureSchema } from "../../../shared/super-ontology/docs-api.generated";
import { ErrorText } from "../shared/ErrorText";
import { useLoad } from "../../hooks/useLoad";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { selectDrives, selectOntologyVersion } from "../../model/superDriveSelectors";
import { isEngineReady } from "../../model/superDriveStateModel";
import { exampleText, isKnownKind, relationTarget, sortedClasses, structureApi, visibleProperties } from "../../model/structureModel";
import { toSuperDriveError, type SuperDriveError } from "../../model/superDriveErrors";

type Edit = { kind: "rename"; classKey: string; property: string | null; current: string } | { kind: "merge"; classKey: string; property: string };

function className(c: StructureClass, t: ReturnType<typeof useLocalization>["t"]): string {
  return isKnownKind(c.kind) ? t(`app.structure.kind.${c.kind}`) : c.label;
}

function PropertyRow({ c, p, classes, busy, onDecide, onEdit }: {
  c: StructureClass; p: StructureProperty; classes: StructureClass[]; busy: boolean;
  onDecide: (op: "accept" | "reject", property: string) => void; onEdit: (edit: Edit) => void;
}) {
  const { t, formatNumber } = useLocalization();
  const anchor = useRef<HTMLButtonElement>(null);
  const [menu, setMenu] = useState(false);
  const rel = relationTarget(p, classes);
  const relLabel = rel ? (rel.kind === "class" ? (isKnownKind(rel.label) ? t(`app.structure.kind.${rel.label}`) : rel.label) : t(`super-drive.entity.type.${rel.label}`)) : null;
  return (
    <div className="so-struct-row" role="row" data-testid="structure-property" data-slug={p.slug} data-type={p.value_type} data-status={p.status}>
      <span className="so-struct-name" role="cell">
        <span className="so-list-title so-truncate" title={p.name}>{p.name}</span>
        <span className="so-list-sub so-truncate">{t(`app.structure.source.${p.source}`)}{p.multi ? ` · ${t("app.structure.multi")}` : ""}{p.required ? ` · ${t("app.structure.required")}` : ""}</span>
      </span>
      <span className="so-struct-type" role="cell">
        <Badge tone={rel ? "accent" : "neutral"}>{t(`app.structure.type.${p.value_type}`)}</Badge>
        {relLabel && <span className="so-struct-rel so-truncate" title={relLabel}>→ {relLabel}</span>}
      </span>
      <span className="so-struct-cover" role="cell">
        <span className="so-struct-bar" aria-hidden="true"><span style={{ width: `${Math.round(p.coverage * 100)}%` }} /></span>
        <span className="so-struct-pct" title={t("app.structure.coverage.files", { count: p.files_with, total: c.files })}>{formatNumber(p.coverage, { style: "percent" })}</span>
      </span>
      <span className="so-struct-examples so-weak so-truncate" role="cell" title={p.examples.join(" · ")}>{p.value_type === "enum" && p.enum_values.length > 0 ? p.enum_values.slice(0, 4).join(" · ") : exampleText(p)}</span>
      <span className="so-struct-actions" role="cell">
        {p.status === "proposed" && c.status === "confirmed" && <Button size="compact" variant="primary" icon={Check} disabled={busy} onClick={() => onDecide("accept", p.slug)}>{t("app.structure.accept")}</Button>}
        {p.status === "confirmed" && <Badge tone="success">{t("app.structure.status.confirmed")}</Badge>}
        <IconButton ref={anchor} icon={MoreHorizontal} label={t("app.structure.more")} variant="ghost" aria-haspopup="menu" aria-expanded={menu} onClick={() => setMenu((v) => !v)} />
        <Menu anchor={anchor} open={menu} onClose={() => setMenu(false)} ariaLabel={t("app.structure.more")} placement="bottom-end">
          <MenuItem icon={Check} label={t("app.structure.accept")} disabled={p.status === "confirmed"} onSelect={() => onDecide("accept", p.slug)} closeMenu={() => setMenu(false)} testId="structure-accept" />
          <MenuItem icon={Pencil} label={t("app.structure.rename")} onSelect={() => onEdit({ kind: "rename", classKey: c.class_key, property: p.slug, current: p.name })} closeMenu={() => setMenu(false)} testId="structure-rename" />
          <MenuItem icon={GitMerge} label={t("app.structure.merge")} disabled={visibleProperties(c).length < 2} onSelect={() => onEdit({ kind: "merge", classKey: c.class_key, property: p.slug })} closeMenu={() => setMenu(false)} testId="structure-merge" />
          <MenuItem icon={X} tone="danger" label={t("app.structure.reject")} onSelect={() => onDecide("reject", p.slug)} closeMenu={() => setMenu(false)} testId="structure-reject" />
        </Menu>
      </span>
    </div>
  );
}

function ClassCard({ c, classes, busy, onDecide, onEdit }: {
  c: StructureClass; classes: StructureClass[]; busy: boolean;
  onDecide: (classKey: string, op: "accept" | "reject", property: string | null) => void; onEdit: (edit: Edit) => void;
}) {
  const { t } = useLocalization();
  const props = visibleProperties(c);
  return (
    <Card className="so-struct-class" data-testid="structure-class" data-class={c.class_key} data-status={c.status}>
      <header className="so-struct-head">
        <span className="so-struct-title">
          <h3 className="so-card-title so-truncate" title={className(c, t)}>{className(c, t)}</h3>
          <span className="so-list-sub so-truncate">{t(`app.structure.shape.${c.shape}`)} · {c.shape === "table" ? t("app.structure.rows", { count: c.rows }) : t("app.structure.files", { count: c.files })}{c.sample_paths[0] ? ` · ${c.sample_paths[0]}` : ""}</span>
        </span>
        <Badge tone={c.status === "confirmed" ? "success" : "warning"}>{t(`app.structure.status.${c.status}`)}</Badge>
        <span className="so-row">
          <IconButton icon={Pencil} label={t("app.structure.renameClass")} variant="ghost" onClick={() => onEdit({ kind: "rename", classKey: c.class_key, property: null, current: className(c, t) })} />
          <Button size="compact" icon={X} disabled={busy} onClick={() => onDecide(c.class_key, "reject", null)}>{t("app.structure.notClass")}</Button>
          {c.status !== "confirmed" && <Button size="compact" variant="primary" icon={Check} disabled={busy} onClick={() => onDecide(c.class_key, "accept", null)} data-testid="structure-confirm">{t("app.structure.confirm")}</Button>}
        </span>
      </header>
      <div className="so-struct-table" role="table" aria-label={className(c, t)}>
        <div className="so-struct-row so-struct-header" role="row">
          <span role="columnheader">{t("app.structure.col.property")}</span>
          <span role="columnheader">{t("app.structure.col.type")}</span>
          <span role="columnheader">{t("app.structure.col.coverage")}</span>
          <span role="columnheader">{t("app.structure.col.examples")}</span>
          <span role="columnheader" className="so-visually-hidden">{t("app.structure.col.actions")}</span>
        </div>
        {props.map((p) => <PropertyRow key={p.slug} c={c} p={p} classes={classes} busy={busy} onDecide={(op, slug) => onDecide(c.class_key, op, slug)} onEdit={onEdit} />)}
      </div>
    </Card>
  );
}

function DriveStructure({ driveId, driveName, showName, ready }: { driveId: string; driveName: string; showName: boolean; ready: boolean }) {
  const { t, formatNumber } = useLocalization();
  const version = useSuperDriveSelector(selectOntologyVersion);
  const result = useLoad(() => structureApi.read(driveId), [driveId, version], { enabled: ready });
  const [override, setOverride] = useState<StructureSchema | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const [edit, setEdit] = useState<Edit | null>(null);
  const [text, setText] = useState("");
  const [into, setInto] = useState("");
  const schema = override ?? result.value;
  const classes = useMemo(() => sortedClasses(schema?.classes ?? []), [schema]);

  const run = async (call: () => ReturnType<typeof structureApi.read>) => {
    setBusy(true); setError(null);
    const r = await call();
    setBusy(false);
    if (r.ok) setOverride(r.value); else setError(toSuperDriveError(r));
    return r.ok;
  };
  const decide = (classKey: string, op: "accept" | "reject", property: string | null) => { void run(() => structureApi.decide({ driveId, op, classKey, property })); };
  const open = (e: Edit) => { setEdit(e); setText(e.kind === "rename" ? e.current : ""); setInto(""); };
  const cls = edit ? classes.find((c) => c.class_key === edit.classKey) : undefined;
  const mergeTargets = edit?.kind === "merge" && cls ? visibleProperties(cls).filter((p) => p.slug !== edit.property) : [];
  const submit = async () => {
    if (!edit) return;
    const ok = await run(() => structureApi.decide(edit.kind === "rename" ? { driveId, op: "rename", classKey: edit.classKey, property: edit.property, name: text.trim() } : { driveId, op: "merge", classKey: edit.classKey, property: edit.property, into: into || mergeTargets[0]?.slug || "" }));
    if (ok) setEdit(null);
  };

  const cov = schema?.coverage;
  return (
    <section className="so-stack" aria-label={driveName} data-testid="structure-drive">
      {showName && <h3 className="so-card-title">{driveName}</h3>}
      {error && <Notice tone="danger" title={t("app.structure.failed")} action={<Button size="compact" onClick={() => setError(null)}>{t("app.common.close")}</Button>}><ErrorText error={error} /></Notice>}
      {result.status === "error" && !schema ? (
        <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={result.reload}>{t("app.common.retry")}</Button>}><ErrorText error={result.error} /></Notice>
      ) : !schema ? (
        <Card className="so-list" aria-busy="true">{Array.from({ length: 3 }, (_, i) => <div key={i} className="so-list-row"><Skeleton width="50%" height={13} /></div>)}</Card>
      ) : classes.length === 0 ? (
        <Card><EmptyState icon={Layers} title={t("app.structure.empty.title")} description={t("app.structure.empty.body")} /></Card>
      ) : (
        <>
          <Card className="so-struct-summary" data-testid="structure-coverage" data-confirmed={cov?.confirmed ?? 0} data-record-files={cov?.record_files ?? 0}>
            <div className="so-struct-figure">
              <span className="so-metric-value">{formatNumber(cov?.share_confirmed ?? 0, { style: "percent" })}</span>
              <span className="so-metric-label">{t("app.structure.coverage")}</span>
            </div>
            <div className="so-struct-meter">
              <ProgressBar value={cov?.share_confirmed ?? 0} label={t("app.structure.coverage")} tone="success" />
              <span className="so-metric-hint">{t("app.structure.coverage.detail", { confirmed: cov?.confirmed ?? 0, total: cov?.record_files ?? 0, version: schema.version })}</span>
            </div>
            <Button size="compact" icon={RefreshCw} loading={busy} onClick={() => { void run(() => structureApi.induce(driveId)); }}>{t("app.structure.refresh")}</Button>
          </Card>
          {classes.map((c) => <ClassCard key={c.class_key} c={c} classes={schema.classes} busy={busy} onDecide={decide} onEdit={open} />)}
        </>
      )}
      {edit && (
        <Dialog title={edit.kind === "rename" ? t("app.structure.rename.title") : t("app.structure.merge.title")} description={edit.kind === "merge" ? t("app.structure.merge.body") : undefined} size="sm" onClose={() => setEdit(null)} closeLabel={t("app.common.close")} testId="structure-dialog"
          footer={<><Button onClick={() => setEdit(null)}>{t("app.common.cancel")}</Button><Button variant="primary" loading={busy} disabled={edit.kind === "rename" ? text.trim() === "" : mergeTargets.length === 0} onClick={() => { void submit(); }} data-testid="structure-dialog-save">{edit.kind === "rename" ? t("app.structure.rename.save") : t("app.structure.merge.save")}</Button></>}>
          {edit.kind === "rename" ? (
            <TextField label={t("app.structure.rename.label")} value={text} onChange={(e) => setText(e.target.value)} maxLength={80} spellCheck={false} autoFocus onKeyDown={(e) => { if (e.key === "Enter" && text.trim() !== "") { e.preventDefault(); void submit(); } }} />
          ) : (
            <Select label={t("app.structure.merge.label")} value={into || mergeTargets[0]?.slug || ""} onChange={(e) => setInto(e.target.value)}>
              {mergeTargets.map((p) => <option key={p.slug} value={p.slug}>{p.name}</option>)}
            </Select>
          )}
        </Dialog>
      )}
    </section>
  );
}

/** Ontology > Structure: what the files of each kind of record share, as columns with types and coverage, confirmable (D93). */
export function Structure() {
  const { t } = useLocalization();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d.id === b[i]?.id));
  if (drives.length === 0) return <Card><EmptyState icon={Layers} title={t("app.structure.empty.title")} description={t("app.structure.empty.body")} /></Card>;
  return (
    <div className="so-stack" data-testid="structure">
      <p className="so-muted">{t("app.structure.intro")}</p>
      {drives.map((d) => <DriveStructure key={d.id} driveId={d.id} driveName={d.name} showName={drives.length > 1} ready={ready} />)}
    </div>
  );
}
