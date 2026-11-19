import { useEffect, useMemo, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, Notice, ProgressBar, Spinner, StatusDot } from "@open-ontology/design-system";
import { ArrowRight, Check, Cloud, FileUp, Folder } from "lucide-react";
import { useApp } from "../hooks/useApp";
import { useErrorText } from "../hooks/useText";
import { ontologyApi } from "../model/superDriveOntologyApi";
import { useSuperDriveActions, useSuperDriveSelector } from "../model/SuperDriveProvider";
import { selectDrives, selectOntologyVersion } from "../model/superDriveSelectors";
import { isEngineReady, presentEngine, type Tone as ModelTone } from "../model/superDriveStateModel";
import { getUploadQueue } from "../model/uploadStore";
import type { EntitySearchRow } from "../model/superDriveOntologyTypes";
import type { Fact } from "../../shared/super-ontology/docs-api.generated";
import type { SuperDriveError } from "../model/superDriveErrors";
import { AppMark } from "../shell/AppMark";
import { Connect } from "./agents/Connect";
import { CloudStorageDialog, useAddFolder } from "./sources/AddSource";
import { fileView } from "./sources/fileView";
import type { Tone } from "@open-ontology/design-system";

const TONE: Record<ModelTone, Tone> = { neutral: "neutral", info: "accent", success: "success", warning: "warning", danger: "danger" };
type Step = 1 | 2 | 3 | 4 | 5;
const STEP_NAMES = ["welcome", "sources", "understanding", "ontology", "agent"] as const;

function Dots({ step }: { step: Step }) {
  const { t } = useLocalization();
  return (
    <div className="so-step-dots" role="img" aria-label={t("app.first.progress", { step, total: 5 })}>
      {([1, 2, 3, 4, 5] as Step[]).map((n) => <span key={n} className="so-step-dot" data-state={n < step ? "done" : n === step ? "current" : undefined} />)}
    </div>
  );
}

/** The five steps of the first run: welcome (engine starts behind it), first material, understanding, the ontology, an agent. */
export function FirstRun() {
  const { t } = useLocalization();
  const app = useApp();
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const [step, setStep] = useState<Step>(1);
  const engine = useSuperDriveSelector((s) => s.engine);
  const ready = isEngineReady(engine);
  const drives = useSuperDriveSelector(selectDrives, (a, b) => a.length === b.length && a.every((d, i) => d === b[i]));
  const files = useSuperDriveSelector((s) => Object.values(s.files));
  const version = useSuperDriveSelector(selectOntologyVersion);
  const presented = presentEngine(engine);
  const started = useRef(false);
  const [error, setError] = useState<SuperDriveError | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const [dragging, setDragging] = useState(false);
  const fileCount = drives.reduce((n, d) => n + (d.files_total ?? 0), 0);

  // The engine starts as soon as the first screen is up; nothing waits for a click.
  useEffect(() => {
    if (started.current || !engine) return;
    started.current = true;
    if (engine.status === "idle" || engine.status === "stopped") void actions.startEngine();
  }, [engine, actions]);

  const addFiles = async (picked: FileList | null) => {
    // The input is cleared right after this call: keep the File objects, not the live list.
    const list = picked ? Array.from(picked) : [];
    if (list.length === 0) return;
    const drive = await actions.ensureMyFiles();
    if (!drive.ok) { setError(drive.error); return; }
    setError(null);
    await getUploadQueue().enqueueFiles(drive.value.id, "", list);
    setStep(3);
  };

  const processing = useMemo(() => drives.reduce((n, d) => n + ["queued", "parsing", "extracting"].reduce((m, key) => m + (typeof d.counts?.[key] === "number" ? (d.counts[key] as number) : 0), 0), 0), [drives]);
  const published = drives.reduce((n, d) => n + (typeof d.counts?.published === "number" ? (d.counts.published as number) : 0), 0);
  const [entities, setEntities] = useState<EntitySearchRow[]>([]);
  const [fact, setFact] = useState<Fact | null>(null);
  useEffect(() => {
    if (step !== 4 || !ready) return;
    void ontologyApi.searchEntities({ q: "", limit: 40 }).then((result) => {
      if (!result.ok) return;
      const people = result.value.items.filter((e) => e.type !== "Document");
      setEntities(people.slice(0, 6));
      const first = people.find((e) => (e.counts?.facts ?? 0) > 0);
      if (first) void ontologyApi.entityDetail({ entityIdOrKey: first.key }).then((detail) => { if (detail.ok) setFact(detail.value.facts?.find((f) => (f.evidence ?? []).length > 0) ?? null); });
    });
  }, [step, ready, version]);
  const understood = drives.length > 0 && processing === 0 && published > 0;
  const { addFolder, error: folderError } = useAddFolder(() => setStep(3));
  const [cloud, setCloud] = useState(false);
  const engineState = ready ? "ready" : !engine || ["starting", "probing", "idle", "stopped", "restarting"].includes(engine.status) ? "starting" : "error";

  useEffect(() => { if (step > 1) window.sessionStorage.setItem("so.firstRun.engaged", "1"); }, [step]);

  const finish = (to?: "ontology" | "agents") => { app.closeFirstRun(true); if (to) app.go(to, to === "agents" ? { agents: "connect" } : { ontology: "entities" }); };

  return (
    <div className="so-firstrun" role="dialog" aria-modal="true" aria-label={t("app.first.title")} data-testid="screen-first-run" data-step={STEP_NAMES[step - 1]}>
      <div className="so-firstrun-head"><span className="so-row"><span style={{ color: "var(--so-accent)" }}><AppMark size={20} /></span><strong>Super Ontology</strong></span><span className="so-row">{step > 1 && <span data-testid="firstrun-progress" data-state={engineState}><StatusDot tone={ready ? "success" : engineState === "error" ? "danger" : "neutral"} label={ready ? t("app.first.engine.ready") : t("app.first.engine.starting")} pulse={engineState === "starting"} /></span>}<Button variant="ghost" onClick={() => finish()}>{t("app.first.skip")}</Button></span></div>
      <div className="so-firstrun-body">
        <div className="so-firstrun-card">
          <Dots step={step} />
          {step === 1 && (
            <>
              <h1 className="so-firstrun-title">{t("app.first.welcome.title")}</h1>
              <p className="so-firstrun-lead">{t("app.first.welcome.body")}</p>
              <Card id="firstrun-engine-state" className="so-form" role="status" aria-live="polite" data-testid="firstrun-progress" data-state={engineState}>
                <div className="so-row">
                  {ready ? <StatusDot tone="success" label={t("app.first.engine.ready")} /> : <><Spinner size={16} label={t("app.first.engine.starting")} /><span>{engine?.status === "starting" || engine?.status === "probing" || !engine ? t("app.first.engine.starting") : t(presented.messageId, presented.values as Record<string, string>)}</span></>}
                </div>
                {!ready && engine && !["starting", "probing", "idle", "stopped", "restarting"].includes(engine.status) && <Notice tone={presented.tone === "danger" ? "danger" : "warning"} title={t(presented.pillId)} action={<Button size="compact" onClick={() => finish()}>{t("app.first.engine.settings")}</Button>}>{t("app.first.engine.help")}</Notice>}
              </Card>
              <div className="so-firstrun-actions"><Button variant="primary" disabled={!ready} aria-describedby="firstrun-engine-state" onClick={() => setStep(2)} data-testid="firstrun-start">{t("app.first.start")}</Button></div>
            </>
          )}
          {step === 2 && (
            <>
              <h1 className="so-firstrun-title">{t("app.first.add.title")}</h1>
              <p className="so-firstrun-lead">{t("app.first.add.body")}</p>
              <div className="so-dropzone" data-testid="firstrun-dropzone" data-active={dragging ? "" : undefined} onDragOver={(e) => { e.preventDefault(); setDragging(true); }} onDragLeave={() => setDragging(false)} onDrop={(e) => { e.preventDefault(); setDragging(false); void addFiles(e.dataTransfer.files); }}>
                <span className="so-list-lead"><FileUp size={20} strokeWidth={1.5} aria-hidden="true" /></span>
                <span className="so-dropzone-title">{t("app.first.add.drop")}</span>
                <span className="so-weak">{t("app.first.add.local")}</span>
                <input ref={input} type="file" multiple hidden onChange={(e) => { void addFiles(e.target.files); e.target.value = ""; }} />
                <Button variant="primary" icon={FileUp} disabled={!ready} onClick={() => input.current?.click()} data-testid="firstrun-pick-files">{t("app.first.add.choose")}</Button>
              </div>
              {error && <Notice tone="danger" title={t("app.sources.actionFailed")}>{errorText(error)}</Notice>}
              {!ready && <p className="so-weak" role="status">{t("app.first.add.waiting")}</p>}
              {folderError && <Notice tone="danger" title={t("app.sources.actionFailed")}>{errorText(folderError)}</Notice>}
              <div className="so-row" data-wrap>
                <span className="so-muted">{t("app.first.add.other")}</span>
                <Button icon={Folder} disabled={!ready} onClick={() => { void addFolder(); }} data-testid="firstrun-pick-folder">{t("app.first.add.folder")}</Button>
                <Button icon={Cloud} disabled={!ready} onClick={() => setCloud(true)} data-testid="firstrun-connect-cloud">{t("app.first.add.cloud")}</Button>
              </div>
              {cloud && <CloudStorageDialog onClose={() => setCloud(false)} onCreated={() => { setCloud(false); setStep(3); }} />}
              <div className="so-firstrun-actions"><Button variant="ghost" onClick={() => setStep(1)}>{t("app.first.back")}</Button>{drives.length > 0 && <Button onClick={() => setStep(3)}>{t("app.first.continue")}</Button>}</div>
            </>
          )}
          {step === 3 && (
            <>
              <h1 className="so-firstrun-title">{t("app.first.reading.title")}</h1>
              <p className="so-firstrun-lead">{t("app.first.reading.body")}</p>
              <Card className="so-list" aria-live="polite">
                {files.length === 0 ? <div className="so-list-row"><Spinner size={16} /><span>{t("app.first.reading.waiting")}</span></div> : files.slice(0, 8).map((file) => {
                  const view = fileView(file);
                  return <div key={file.id} className="so-list-row" data-testid="firstrun-file-row" data-name={file.name ?? file.path} data-state={file.state}><span className="so-list-text"><span className="so-list-title so-truncate">{file.name ?? file.path}</span></span><StatusDot tone={TONE[view.tone]} label={t(view.messageId, view.values as Record<string, string>)} pulse={view.busy} /></div>;
                })}
              </Card>
              <ProgressBar label={t("app.first.reading.title")} value={fileCount > 0 ? published / fileCount : null} />
              <div className="so-firstrun-actions"><Button variant="primary" icon={ArrowRight} onClick={() => setStep(4)} data-testid="firstrun-see-ontology">{t("app.first.reading.done")}</Button>{!understood && <span className="so-weak">{t("app.first.reading.leave")}</span>}</div>
            </>
          )}
          {step === 4 && (
            <>
              <h1 className="so-firstrun-title">{t("app.first.see.title")}</h1>
              <p className="so-firstrun-lead">{t("app.first.see.body")}</p>
              {entities.length > 0 && <div className="so-pill-row" aria-label={t("app.home.metric.entities")}>{entities.map((e) => <span key={e.id} className="so-chip">{e.name}</span>)}</div>}
              {fact && (
                <Card className="so-form" data-testid="first-fact">
                  <span className="so-list-title">{fact.subject.name} · {fact.predicate.replace(/_/g, " ")} · {fact.object?.name ?? String(Object.values(fact.value ?? {})[0] ?? "")}</span>
                  <blockquote className="so-quote">{fact.evidence?.[0]?.quote ?? fact.evidence?.[0]?.src.replace(/^\[src:|\]$/g, "")}</blockquote>
                  <span className="so-list-sub so-mono">{fact.evidence?.[0]?.path}</span>
                </Card>
              )}
              <div className="so-firstrun-actions"><Button variant="primary" icon={ArrowRight} onClick={() => setStep(5)} data-testid="firstrun-connect-agent">{t("app.first.see.agent")}</Button><Button variant="ghost" onClick={() => finish("ontology")} data-testid="firstrun-open-ontology">{t("app.first.see.open")}</Button></div>
            </>
          )}
          {step === 5 && (
            <>
              <h1 className="so-firstrun-title">{t("app.first.agent.title")}</h1>
              <p className="so-firstrun-lead">{t("app.first.agent.body")}</p>
              <Connect compact />
              <div className="so-firstrun-actions"><Button variant="primary" icon={Check} onClick={() => finish("ontology")}>{t("app.first.done")}</Button></div>
            </>
          )}
        </div>
      </div>
    </div>
  );
}
