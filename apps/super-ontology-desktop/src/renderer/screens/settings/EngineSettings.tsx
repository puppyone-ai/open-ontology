import { useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, DefinitionList, Notice, Section, Segmented, StatusDot, Switch } from "@open-ontology/design-system";
import { FolderOpen, Play, RefreshCw, RotateCw, ScrollText, Square } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { useSuperDriveActions, useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { COMPONENT_NAMES, COMPONENT_TONES, isEngineReady, presentEngine, type Tone as ModelTone } from "../../model/superDriveStateModel";
import { formatBytes } from "../../model/format";
import type { Tone } from "@open-ontology/design-system";

const TONE: Record<ModelTone, Tone> = { neutral: "neutral", info: "accent", success: "success", warning: "warning", danger: "danger" };

/** What the engine is doing, whether this computer can run it, and the controls that fix it. The primary action is chosen from the state, not here. */
export function EngineSettings() {
  const t$ = useLocalization();
  const { t } = t$;
  const actions = useSuperDriveActions();
  const errorText = useErrorText();
  const engine = useSuperDriveSelector((s) => s.engine);
  const action = useSuperDriveSelector((s) => s.engineAction);
  const [busyId, setBusyId] = useState<string | null>(null);
  const presented = presentEngine(engine);
  const ready = isEngineReady(engine);
  const preflight = engine?.preflight ?? null;
  const components = (engine?.health?.components ?? {}) as Record<string, { status?: string } | undefined>;
  const run = async (id: string, fn: () => Promise<unknown>) => { setBusyId(id); await fn(); setBusyId(null); };
  const running = presented.busy || action.busy;
  const status = t(presented.messageId, presented.values as Record<string, string>);

  return (
    <div className="so-stack" data-testid="engine-settings">
      <Card className="so-form">
        <div className="so-row" data-wrap>
          <StatusDot tone={TONE[presented.tone]} label={status} pulse={presented.busy} />
          <span className="so-grow" />
          {presented.primary === "start" || engine?.status === "stopped" || engine?.status === "idle" ? <Button variant="primary" icon={Play} loading={running} onClick={() => { void run("start", () => actions.startEngine()); }}>{t("app.engine.start")}</Button> : null}
          {presented.primary === "checkEnvironment" && engine?.status !== "idle" ? <Button icon={RefreshCw} loading={running} onClick={() => { void run("check", () => actions.runPreflight()); }}>{t("app.engine.check")}</Button> : null}
          {ready && <><Button icon={RotateCw} loading={busyId === "restart"} onClick={() => { void run("restart", () => actions.restartEngine()); }}>{t("app.engine.restart")}</Button><Button icon={Square} loading={busyId === "stop"} onClick={() => { void run("stop", () => actions.stopEngine()); }}>{t("app.engine.stop")}</Button></>}
          {!ready && presented.primary && !["start", "checkEnvironment"].includes(presented.primary) && <Button icon={RefreshCw} loading={running} onClick={() => { void run("retry", () => actions.runPreflight()); }}>{t("app.engine.retry")}</Button>}
        </div>
        {action.error && <Notice tone="danger" title={t("app.engine.failed")}>{errorText(action.error)}</Notice>}
        {engine?.status === "oo_missing" && <Notice tone="warning" title={t("app.engine.oo.title")} action={<Button size="compact" icon={FolderOpen} onClick={() => { void actions.chooseOoExecutable(); }}>{t("app.engine.oo.choose")}</Button>}>{t("app.engine.oo.body")}</Notice>}
        {(engine?.status === "docker_missing" || engine?.status === "docker_stopped") && <Notice tone="warning" title={t(engine.status === "docker_missing" ? "app.engine.docker.missing" : "app.engine.docker.stopped")}>{t("app.engine.docker.body")}</Notice>}
        {engine?.status === "crashed" && engine.code && <Notice tone="danger" title={t("app.engine.crashed")}>{errorText({ code: engine.code, retryable: false, details: {} })}</Notice>}
      </Card>

      {ready && Object.keys(components).length > 0 && (
        <Section title={t("app.engine.components")}>
          <Card className="so-list">
            {COMPONENT_NAMES.filter((name) => components[name]).map((name) => {
              const status = components[name]?.status ?? "ok";
              const tone = (COMPONENT_TONES as Record<string, ModelTone>)[status] ?? "neutral";
              return (
                <div key={name} className="so-list-row"><span className="so-list-text"><span className="so-list-title">{t(`super-drive.engine.component.${name}.label`)}</span></span><StatusDot tone={TONE[tone]} label={t(`super-drive.engine.component.status.${status}`)} /></div>
              );
            })}
          </Card>
        </Section>
      )}

      {preflight && (
        <Section title={t("app.engine.setup")}>
          <Card className="so-form">
            <DefinitionList items={[
              { label: t("app.engine.setup.oo"), value: preflight.oo.found ? `${preflight.oo.version ?? ""} · ${preflight.oo.source === "user" ? t("app.engine.setup.oo.chosen") : t("app.engine.setup.oo.path")}` : t("app.engine.setup.missing") },
              { label: t("app.engine.setup.docker"), value: preflight.docker.installed ? (preflight.docker.running ? t("app.engine.setup.docker.running", { version: preflight.docker.version ?? "" }) : t("app.engine.setup.docker.stopped")) : t("app.engine.setup.docker.none") },
              { label: t("app.engine.setup.runtime"), value: preflight.runtime ? t(`app.engine.runtime.${preflight.runtime}`) : "–" },
              { label: t("app.engine.setup.disk"), value: preflight.diskFreeBytes === null ? "–" : t("app.engine.setup.disk.free", { size: formatBytes(preflight.diskFreeBytes, t$) }) },
            ]} />
            <div className="so-stack"><span className="so-field-label">{t("app.engine.runtime.choose")}</span>
              <Segmented<"auto" | "docker" | "lite"> label={t("app.engine.runtime.choose")} value={preflight.runtimeChoice ?? "auto"} onChange={(value) => { void actions.setRuntime(value); }} options={[{ value: "auto", label: t("app.engine.runtime.auto") }, { value: "docker", label: t("app.engine.runtime.docker") }, { value: "lite", label: t("app.engine.runtime.lite") }]} />
              <p className="so-weak">{t("app.engine.runtime.help")}</p>
            </div>
          </Card>
        </Section>
      )}

      <Section title={t("app.engine.options")}>
        <Card className="so-form">
          <Switch checked={engine?.keepEngineOnQuit ?? false} onChange={(value) => { void actions.setKeepEngine(value); }} label={t("app.engine.keep")} description={t("app.engine.keep.help")} />
          <div className="so-row" data-wrap>
            <Button icon={ScrollText} onClick={() => { void actions.revealEngineFolder("logs"); }}>{t("app.engine.logs")}</Button>
            <Button icon={FolderOpen} onClick={() => { void actions.revealEngineFolder("state"); }}>{t("app.engine.state")}</Button>
          </div>
        </Card>
      </Section>
    </div>
  );
}
