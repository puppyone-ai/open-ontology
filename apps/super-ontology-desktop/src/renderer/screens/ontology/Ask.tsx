import { useRef, useState, type KeyboardEvent, type ReactNode } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, Card, EmptyState, Icon, Notice, Spinner, TextArea } from "@open-ontology/design-system";
import { Check, Copy, Eraser, FileText, MessageSquareText, Square, Table2 } from "lucide-react";
import type { AskSource, AskStep } from "../../../shared/super-ontology/bridge";
import { useApp } from "../../hooks/useApp";
import { askStore, useAskTurns, type AskTurn } from "../../model/askStore";
import { copyText } from "../../model/format";
import { queryDraft, rowQuery } from "../../model/queryDraft";
import { ErrorText } from "../shared/ErrorText";
import { CliCommand } from "../shared/Cli";
import { toSuperDriveError } from "../../model/superDriveErrors";

/** What a quiet progress line shows of a step's main argument: an entity URN reads as its last part. */
const shown = (step: { tool: string; arg: string }) => (step.tool === "entity" ? step.arg.slice(step.arg.lastIndexOf("/") + 1) : step.tool === "read_file" ? step.arg.split("/").pop() ?? step.arg : step.arg);

export function StepLine({ step }: { step: AskStep }) {
  const { t } = useLocalization();
  return (
    <li className="so-ask-step" data-testid="ask-step" data-tool={step.tool} data-state={step.state ?? "done"}>
      <span className="so-ask-step-mark" aria-hidden="true">{step.state === "running" ? <Spinner size={12} /> : <span className="so-ask-step-dot" />}</span>
      <span className="so-truncate">{t(`app.ask.step.${step.tool}`, { arg: shown(step) })}</span>
      {step.state === "failed" && <span className="so-weak">{t("app.ask.step.failed")}</span>}
    </li>
  );
}

const sourceName = (source: AskSource) => (source.kind === "file" ? source.path.slice(source.path.lastIndexOf("/") + 1) : source.kind === "row" ? source.table : source.sql.replace(/\s+/gu, " ").slice(0, 40));

/** A cited source as a chip: a file opens in Sources, a row or a query opens in Query with the SQL filled in. */
export function SourceChip({ source, onOpenFile, onOpenQuery }: { source: AskSource; onOpenFile: (fileId: string) => void; onOpenQuery: (sql: string) => void }) {
  const { t } = useLocalization();
  const number = source.id.slice(1);
  const human = source.kind === "file" && source.anchor && !/^blk_/u.test(source.anchor) ? source.anchor : "";
  const anchor = human ? `#${human}` : source.kind === "row" ? `${source.keyColumn}=${source.pk}` : "";
  const sql = source.kind === "query" ? source.sql : source.kind === "row" ? rowQuery(source.table, source.keyColumn, source.pk) : "";
  const fileId = source.kind === "file" ? source.fileId : null;
  const label = source.kind === "file" ? t("app.ask.source.file", { path: source.path }) : source.kind === "row" ? t("app.ask.source.row", { table: source.table }) : t("app.ask.source.query");
  const body = (
    <>
      <span className="so-ask-cite-number so-mono" aria-hidden="true">{number}</span>
      <Icon icon={source.kind === "file" ? FileText : Table2} size={16} className="so-source-chip-icon" />
      <span className="so-source-chip-name so-truncate">{sourceName(source)}</span>
      {anchor && <span className="so-source-chip-anchor so-mono so-truncate" title={anchor}>{anchor}</span>}
    </>
  );
  if (source.kind === "file" && !fileId) return <span className="so-source-chip so-ask-chip" data-testid="ask-source" data-kind="file" data-id={source.id} title={source.path}>{body}</span>;
  return (
    <button type="button" className="so-source-chip so-ask-chip" data-testid="ask-source" data-kind={source.kind} data-id={source.id} aria-label={label} title={source.kind === "file" ? `${source.path}${source.anchor ? ` ${source.anchor}` : ""}${source.blockId ? ` (${source.blockId})` : ""}` : sql}
      onClick={() => { if (fileId) onOpenFile(fileId); else onOpenQuery(sql); }}>
      {body}
    </button>
  );
}

const CITE_RUN = /((?:\[S\d+(?:, S\d+)*\])+)([.,;:!?。，；：！？）)」』]*)/gu;
const CITE_ONE = /\[(S\d+(?:, S\d+)*)\]/gu;

/** The answer as paragraphs; a run of citation tags becomes the small numbers of its chips, kept on one line with the punctuation that follows. */
export function AnswerText({ text }: { text: string }) {
  return (
    <div className="so-ask-answer-text" data-testid="ask-answer-text">
      {text.split(/\n{1,}/u).filter((line) => line.trim() !== "").map((line, index) => {
        const parts: ReactNode[] = [];
        let last = 0;
        for (const match of line.matchAll(CITE_RUN)) {
          parts.push(line.slice(last, match.index));
          const ids = [...match[1]!.matchAll(CITE_ONE)].flatMap((m) => m[1]!.split(", ").map((id) => id.slice(1)));
          parts.push(<span key={`${index}-${match.index}`} className="so-ask-cites">{ids.map((id, i) => <span key={i} className="so-ask-cite so-mono">{id}</span>)}{match[2]}</span>);
          last = match.index + match[0].length;
        }
        parts.push(line.slice(last));
        return <p key={index}>{parts}</p>;
      })}
    </div>
  );
}

function Failure({ turn }: { turn: AskTurn }) {
  const { t } = useLocalization();
  const app = useApp();
  const retry = <Button size="compact" onClick={() => { void askStore.ask(turn.question); }}>{t("app.common.retry")}</Button>;
  const settings = (tab: "model" | "engine", label: string) => <Button size="compact" onClick={() => app.go("settings", { settings: tab })}>{label}</Button>;
  if (turn.failure) return <Notice tone="danger" title={t("app.ask.error.bridge.title")} action={retry} testId="ask-error"><ErrorText error={toSuperDriveError({ ok: false, error: turn.failure })} /></Notice>;
  switch (turn.outcome?.status) {
    case "no_model": return <Notice tone="warning" title={t("app.ask.error.no_model.title")} action={settings("model", t("app.ask.action.model"))} testId="ask-error" ><span data-state="no_model">{t("app.ask.error.no_model.body")}</span></Notice>;
    case "key_unreadable": return <Notice tone="warning" title={t("app.ask.error.key_unreadable.title")} action={settings("model", t("app.ask.action.model"))} testId="ask-error"><span data-state="key_unreadable">{t("app.ask.error.key_unreadable.body")}</span></Notice>;
    case "model_rejected": return <Notice tone="warning" title={t("app.ask.error.model_rejected.title")} action={settings("model", t("app.ask.action.model"))} testId="ask-error"><span data-state="model_rejected">{t("app.ask.error.model_rejected.body")}</span></Notice>;
    case "model_timeout": return <Notice tone="warning" title={t("app.ask.error.model_timeout.title")} action={retry} testId="ask-error"><span data-state="model_timeout">{t("app.ask.error.model_timeout.body")}</span></Notice>;
    case "model_failed": return <Notice tone="warning" title={t("app.ask.error.model_failed.title")} action={retry} testId="ask-error"><span data-state="model_failed">{t("app.ask.error.model_failed.body")}</span></Notice>;
    case "engine_down": return <Notice tone="warning" title={t("app.ask.error.engine_down.title")} action={settings("engine", t("app.ask.action.engine"))} testId="ask-error"><span data-state="engine_down">{t("app.ask.error.engine_down.body")}</span></Notice>;
    default: return null;
  }
}

function Turn({ turn }: { turn: AskTurn }) {
  const { t } = useLocalization();
  const app = useApp();
  const [copied, setCopied] = useState(false);
  const outcome = turn.outcome;
  const steps = outcome?.steps ?? turn.steps;
  const openQuery = (sql: string) => { queryDraft.set(sql); app.go("ontology", { ontology: "query" }); };
  const copy = async () => { await copyText(outcome?.answer ?? ""); setCopied(true); window.setTimeout(() => setCopied(false), 1500); };
  const answered = outcome?.status === "answered";
  const unanswerable = outcome?.status === "unanswerable";
  return (
    <li className="so-ask-turn" data-testid="ask-turn" data-state={turn.state} data-status={outcome?.status ?? (turn.failure ? "failed" : "running")}>
      <Card className="so-ask-card">
        <p className="so-ask-question" data-testid="ask-question">{turn.question}</p>
        {turn.state === "running" && (
          <div className="so-ask-progress" data-testid="ask-progress" role="status" aria-live="polite">
            <p className="so-ask-working"><Spinner size={12} /><span>{t("app.ask.working")}</span></p>
            {steps.length > 0 && <ul className="so-ask-steps">{steps.map((step) => <StepLine key={step.n} step={step} />)}</ul>}
          </div>
        )}
        {answered && (
          <div className="so-ask-answer" data-testid="ask-answer">
            <AnswerText text={outcome.answer ?? ""} />
            {outcome.uncited && <p className="so-ask-uncited so-weak" data-testid="ask-uncited">{t("app.ask.uncited")}</p>}
            {(outcome.sources?.length ?? 0) > 0 && (
              <div className="so-ask-sources" data-testid="ask-sources" role="group" aria-label={t("app.ask.sources")}>
                {outcome.sources!.map((source) => <SourceChip key={source.id} source={source} onOpenFile={app.openFile} onOpenQuery={openQuery} />)}
              </div>
            )}
          </div>
        )}
        {unanswerable && (
          <div className="so-ask-unanswerable" data-testid="ask-unanswerable">
            <p className="so-ask-unanswerable-title"><Icon icon={Square} size={16} className="so-ask-unanswerable-mark" />{t("app.ask.unanswerable.title")}</p>
            {outcome.reason && <p className="so-ask-reason" data-testid="ask-reason">{outcome.reason}</p>}
            <p className="so-weak">{t("app.ask.unanswerable.hint")}{outcome.budget ? ` ${t("app.ask.budget")}` : ""}</p>
            <p className="so-ask-searched-head">{t("app.ask.searched")}</p>
            {(outcome.searched?.length ?? 0) > 0
              ? <ul className="so-ask-steps" data-testid="ask-searched">{outcome.searched!.map((s, i) => <StepLine key={i} step={{ n: i + 1, tool: s.tool, arg: s.arg, state: "done" }} />)}</ul>
              : <p className="so-weak">{t("app.ask.searched.none")}</p>}
          </div>
        )}
        {turn.state === "done" && <Failure turn={turn} />}
        {outcome?.status === "cancelled" && <p className="so-weak" data-testid="ask-cancelled">{t("app.ask.cancelled")}</p>}
        {answered && steps.length > 0 && (
          <details className="so-ask-details">
            <summary>{t("app.ask.steps", { count: steps.length })}</summary>
            <ul className="so-ask-steps">{steps.map((step) => <StepLine key={step.n} step={{ ...step, state: step.ok === false ? "failed" : "done" }} />)}</ul>
          </details>
        )}
        {(answered || unanswerable) && (
          <div className="so-ask-actions">
            {answered && <Button size="compact" variant="ghost" icon={copied ? Check : Copy} onClick={() => { void copy(); }} data-testid="ask-copy">{copied ? t("app.common.copied") : t("app.ask.copy")}</Button>}
            {outcome.cli && <CliCommand command={outcome.cli} testId="ask-cli" />}
          </div>
        )}
      </Card>
    </li>
  );
}

/** Ontology › Ask: type a question, read an answer that cites the files and rows it came from, or be told the data has none. */
export function Ask() {
  const { t } = useLocalization();
  const turns = useAskTurns();
  const [text, setText] = useState("");
  const box = useRef<HTMLTextAreaElement>(null);
  const running = turns.some((turn) => turn.state === "running");
  const submit = () => {
    if (!text.trim() || running) return;
    const question = text;
    setText("");
    void askStore.ask(question);
  };
  const onKey = (event: KeyboardEvent<HTMLTextAreaElement>) => { if ((event.metaKey || event.ctrlKey) && event.key === "Enter") { event.preventDefault(); submit(); } };
  return (
    <div className="so-ask" data-testid="ontology-ask">
      <form className="so-ask-composer" onSubmit={(event) => { event.preventDefault(); submit(); }}>
        <TextArea ref={box} label={t("app.ask.input.label")} labelHidden value={text} onChange={(event) => setText(event.target.value)} onKeyDown={onKey} rows={2}
          placeholder={t("app.ask.input.placeholder")} helper={t("app.ask.input.help")} maxLength={2000} data-testid="ask-input" />
        <div className="so-ask-composer-actions">
          {running
            ? <Button variant="secondary" icon={Square} onClick={() => { void askStore.cancel(); }} data-testid="ask-stop">{t("app.ask.stop")}</Button>
            : <Button variant="primary" icon={MessageSquareText} type="submit" disabled={text.trim() === ""} data-testid="ask-send">{t("app.ask.send")}</Button>}
          {turns.length > 0 && !running && <Button variant="ghost" icon={Eraser} onClick={() => askStore.clear()} data-testid="ask-clear">{t("app.ask.clear")}</Button>}
        </div>
      </form>
      {turns.length === 0
        ? <Card><EmptyState icon={MessageSquareText} title={t("app.ask.empty.title")} description={t("app.ask.empty.body")} compact /></Card>
        : <ol className="so-ask-turns" data-testid="ask-turns">{[...turns].reverse().map((turn) => <Turn key={turn.id} turn={turn} />)}</ol>}
    </div>
  );
}
