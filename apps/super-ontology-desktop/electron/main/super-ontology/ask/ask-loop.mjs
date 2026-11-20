import { describeCall } from "./ask-tools.mjs";
import { ModelError } from "./model-client.mjs";

/** At most this many tool calls per question (the budget lane V's agent QA ran with). */
export const MAX_CALLS = 12;

export const SYSTEM_PROMPT = `You answer questions about the user's own documents (PDF, Word, Excel, slides, audio transcripts, images, text) and, when one is connected, their database. Your tools are read-only and return what the data says; each source is tagged [S1], [S2] ...

Rules:
- Answer only from what the tools returned. Never use outside knowledge for facts about the user's data, never guess a number, a name or a date.
- Cite every claim with the tags of the sources it comes from, inline, for example "The contract ends in March [S2]". Use only tags a tool printed.
- Reply in the language of the question. The data may be in another language: if a search is empty, search again with the other language's form of the key words (a name in Chinese characters or in pinyin, the English or Chinese term for a concept), a shorter word or a synonym, before you decide the data does not have it.
- Start with search; use find_text for passages, read_file for context, entity for the facts of one entity. Use schema and sql only for database questions. You can make at most {max} tool calls.
- Whenever the question is about an owner, a date, a number or a status, call disagreements for the subject before you answer. If it reports conflicting current values, say plainly that the sources disagree, give each value, and cite the source of each. Never say the sources agree unless disagreements found nothing.
- If the data does not contain the answer, or the question assumes something the data contradicts, begin your final reply with NOT_IN_DATA: followed by one plain sentence saying what you looked for and did not find. Do not offer a guess after it.
- Keep answers short and direct: the answer first, then the key figures or names.`;

const CITE = /\[\s*(S\d+(?:\s*[,;]\s*S\d+)*)\s*\]/gu;

function readCitations(text, registry) {
  const ids = [];
  let invalid = 0;
  const cleaned = text.replace(CITE, (whole, inner) => {
    const kept = [];
    for (const id of inner.split(/[,;]/u).map((s) => s.trim())) {
      if (registry.get(id)) { kept.push(id); if (!ids.includes(id)) ids.push(id); } else invalid += 1;
    }
    return kept.length ? `[${kept.join(", ")}]` : "";
  });
  return { text: cleaned.replace(/[ \t]+\n/gu, "\n").replace(/ {2,}/gu, " ").replace(/ ([.,;:!?。，；：！？])/gu, "$1").trim(), ids, invalid };
}

/** The tags a tool printed number every source it returned; the answer shows only the cited ones, as 1, 2, 3 in the order they are first cited. */
function renumber(text, ids) {
  const order = new Map(ids.map((id, i) => [id, `S${i + 1}`]));
  return { text: text.replace(CITE, (_whole, inner) => `[${inner.split(/[,;]/u).map((id) => order.get(id.trim()) ?? id.trim()).join(", ")}]`), order };
}

/** The `oo` commands that do what the steps did, for "equivalent command". */
export function equivalentCommands(steps) {
  const q = (s) => `"${String(s).replace(/(["\\$`])/gu, "\\$1")}"`;
  const lines = [];
  for (const s of steps) {
    const line = s.tool === "search" || s.tool === "find_text" ? `oo ontology search ${q(s.arg)}`
      : s.tool === "entity" ? `oo ontology entity ${q(s.arg)}`
        : s.tool === "sql" ? `oo ontology query ${q(s.arg)}`
          : s.tool === "disagreements" ? `oo docs entity search ${q(s.arg)}`
          : s.tool === "schema" ? "oo ontology schema"
            : s.tool === "read_file" ? `oo knowledge cat ${q(s.arg)}` : null;
    if (line && !lines.includes(line)) lines.push(line);
  }
  return lines.slice(0, 8).join("\n");
}

/**
 * The bounded tool-calling loop. `model.chat` and `tools.run` are injected (a fake model and fake tools in the tests).
 * Never throws for a model failure it can name: that is `{ status: "model_error", kind }`.
 */
export async function runAskLoop({ question, history = [], model, tools, registry, emit = () => {}, signal, maxCalls = MAX_CALLS }) {
  const system = SYSTEM_PROMPT.replace("{max}", String(maxCalls));
  const messages = [];
  for (const turn of history.slice(-4)) {
    messages.push({ role: "user", text: turn.question });
    messages.push({ role: "assistant", text: turn.answer });
  }
  messages.push({ role: "user", text: question });
  const steps = [];
  let calls = 0;
  let nudged = false;
  let finalText = "";
  let exhausted = false;
  try {
    for (let turn = 0; turn < maxCalls + 3; turn += 1) {
      if (signal?.aborted) return { status: "cancelled", steps };
      const offer = calls < maxCalls ? tools.definitions : [];
      const reply = await model.chat({ system, messages, tools: offer, signal });
      if (reply.toolCalls.length === 0 || offer.length === 0) {
        if (reply.toolCalls.length === 0 && calls === 0 && !nudged && reply.text.trim() && !/^\s*NOT_IN_DATA/iu.test(reply.text)) {
          nudged = true;
          messages.push({ role: "assistant", text: reply.text });
          messages.push({ role: "user", text: "Check the data with your tools before you answer. If it is not there, reply with NOT_IN_DATA." });
          continue;
        }
        finalText = reply.text;
        if (reply.toolCalls.length > 0) exhausted = true;
        break;
      }
      messages.push({ role: "assistant", text: reply.text, toolCalls: reply.toolCalls });
      const results = [];
      for (const call of reply.toolCalls) {
        if (calls >= maxCalls) { results.push({ id: call.id, name: call.name, content: "error: the tool budget for this question is used; answer from what you have" }); continue; }
        calls += 1;
        const described = describeCall(call.name, call.args ?? {});
        const step = { n: calls, ...described };
        steps.push(step);
        emit({ type: "step", ...step, state: "running" });
        const content = await tools.run(call.name, call.args ?? {}, signal);
        const failed = content.startsWith("error");
        step.ok = !failed;
        emit({ type: "step", ...step, state: failed ? "failed" : "done" });
        results.push({ id: call.id, name: call.name, content });
      }
      messages.push({ role: "tool", results });
      if (calls >= maxCalls) messages.push({ role: "user", text: "The tool budget is used. Give your final answer now from what the tools returned, or NOT_IN_DATA." });
    }
  } catch (error) {
    if (error instanceof ModelError) return error.kind === "cancelled" ? { status: "cancelled", steps } : { status: "model_error", kind: error.kind, httpStatus: error.httpStatus, steps };
    throw error;
  }
  const searched = steps.map(({ tool, arg }) => ({ tool, arg }));
  const text = finalText.trim();
  const out = { steps, searched, cli: equivalentCommands(steps) };
  if (!text) return { ...out, status: "unanswerable", answer: "", reason: "", budget: exhausted };
  const declined = /^\s*NOT_IN_DATA\b[\s:.\-–—]*/iu.exec(text);
  if (declined) return { ...out, status: "unanswerable", answer: "", reason: text.slice(declined[0].length).trim(), budget: false };
  const { text: cited, ids, invalid } = readCitations(text, registry);
  const { text: answer, order } = renumber(cited, ids);
  return { ...out, status: "answered", answer, sources: ids.map((id) => ({ ...registry.get(id), id: order.get(id) })), uncited: ids.length === 0, invalidCitations: invalid, budget: exhausted };
}
