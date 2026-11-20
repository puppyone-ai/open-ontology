import { useSyncExternalStore } from "react";
import { invoke } from "./bridgeInvoke";
import { superDriveApi } from "./superDriveApi";
import type { AskOutcome, AskStep, SuperOntologyFailure } from "../../shared/super-ontology/bridge";

/**
 * The Ask session: the questions asked since the app opened, each with the steps it took and what came back. Kept in memory only
 * (never written to disk). A new question while one runs waits for it; "Stop" cancels the one that runs.
 */
export type AskTurn = {
  id: string;
  question: string;
  state: "running" | "done";
  steps: AskStep[];
  /** Null while running. A failed call to the main process becomes `{ status: "model_failed" }`-like `engine_down` or `failed`. */
  outcome: AskOutcome | null;
  /** A bridge failure (not a model one): the code, for the calm inline state. */
  failure: SuperOntologyFailure["error"] | null;
};

type Listener = () => void;
const HISTORY_SENT = 4;
const SESSION_LIMIT = 30;

let turns: AskTurn[] = [];
const listeners = new Set<Listener>();
let unsubscribe: (() => void) | null = null;
let counter = 0;

const emit = () => { for (const listener of listeners) listener(); };
const patch = (id: string, change: (turn: AskTurn) => AskTurn) => { turns = turns.map((turn) => (turn.id === id ? change(turn) : turn)); emit(); };

function ensureSubscribed(): void {
  if (unsubscribe) return;
  unsubscribe = superDriveApi.subscribePushes((push) => {
    if (push.channel !== "ask-progress") return;
    const { askId, ...step } = push.payload;
    patch(askId, (turn) => {
      const known = turn.steps.some((s) => s.n === step.n);
      const next = { n: step.n, tool: step.tool, arg: step.arg, state: step.state };
      return { ...turn, steps: known ? turn.steps.map((s) => (s.n === step.n ? next : s)) : [...turn.steps, next] };
    });
  });
}

export const askStore = {
  subscribe(listener: Listener) { listeners.add(listener); return () => { listeners.delete(listener); }; },
  snapshot: () => turns,
  running: () => turns.some((turn) => turn.state === "running"),
  /** Starts a question; resolves when the answer (or the calm failure) is in the store. */
  async ask(question: string): Promise<void> {
    const text = question.trim();
    if (!text || askStore.running()) return;
    ensureSubscribed();
    counter += 1;
    const id = `ask-${Date.now().toString(36)}${counter.toString(36).padStart(3, "0")}`;
    const history = turns.filter((turn) => turn.outcome?.status === "answered" || turn.outcome?.status === "unanswerable").slice(-HISTORY_SENT)
      .map((turn) => ({ question: turn.question, answer: turn.outcome?.status === "answered" ? (turn.outcome.answer ?? "").slice(0, 3900) : (turn.outcome?.reason ?? "").slice(0, 3900) }));
    const fresh: AskTurn = { id, question: text, state: "running", steps: [], outcome: null, failure: null };
    turns = [...turns, fresh].slice(-SESSION_LIMIT);
    emit();
    const result = await invoke("ask-run", { askId: id, question: text, ...(history.length ? { history } : {}) });
    patch(id, (turn) => (result.ok ? { ...turn, state: "done", outcome: result.value } : { ...turn, state: "done", failure: result.error }));
  },
  async cancel(): Promise<void> {
    const turn = turns.find((candidate) => candidate.state === "running");
    if (turn) await invoke("ask-cancel", { askId: turn.id });
  },
  clear() { turns = turns.filter((turn) => turn.state === "running"); emit(); },
  /** Tests only. */
  reset() { turns = []; counter = 0; unsubscribe?.(); unsubscribe = null; emit(); },
};

export function useAskTurns(): AskTurn[] {
  return useSyncExternalStore(askStore.subscribe, askStore.snapshot, askStore.snapshot);
}
