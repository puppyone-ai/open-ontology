// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import type { ReactNode } from "react";
import { AppProvider, useApp } from "../../../../src/renderer/hooks/useApp";
import { installBridgeReader } from "../../../../src/renderer/model/bridgeInvoke";
import { askStore } from "../../../../src/renderer/model/askStore";
import { queryDraft, rowQuery } from "../../../../src/renderer/model/queryDraft";
import { Ask, AnswerText } from "../../../../src/renderer/screens/ontology/Ask";
import type { AskOutcome } from "../../../../src/shared/super-ontology/bridge";
import { catalog } from "../../../support/locales";

const messages = mergeCatalogNamespaces({ app: catalog("en", "app"), "super-drive": catalog("en", "super-drive") });

/** Shows where the app was sent, so a chip click can be asserted. */
function Where() {
  const app = useApp();
  return <output data-testid="where" data-section={app.section} data-tab={app.ontologyTab} data-focus={app.focusFile ?? ""} />;
}
const Shell = ({ children }: { children: ReactNode }) => <TestLocalizationProvider messages={messages}><AppProvider>{children}<Where /></AppProvider></TestLocalizationProvider>;

type Pending = { resolve: (value: AskOutcome) => void };
function installBridge(handler: (request: { askId: string; question: string; history?: unknown[] }) => AskOutcome | Pending) {
  let push: ((p: { channel: string; payload: unknown }) => void) | null = null;
  const calls: unknown[] = [];
  installBridgeReader(() => ({
    invoke: async (channel: string, request: { askId: string; question: string }) => {
      calls.push([channel, request]);
      if (channel === "ask-cancel") return { ok: true, value: { cancelled: true } };
      const out = handler(request as never);
      if ("resolve" in out) return new Promise((resolve) => { out.resolve = (value) => resolve({ ok: true, value }); });
      return { ok: true, value: out };
    },
    subscribe: (callback: never) => { push = callback; return () => { push = null; }; },
  } as never));
  return { calls, push: (payload: unknown) => act(() => push?.({ channel: "ask-progress", payload })) };
}

const ask = async (text: string) => {
  fireEvent.change(screen.getByTestId("ask-input"), { target: { value: text } });
  fireEvent.click(screen.getByTestId("ask-send"));
};

beforeEach(() => { window.localStorage.clear(); askStore.reset(); });
afterEach(() => { cleanup(); installBridgeReader(() => null); });

describe("Ask: citations", () => {
  it("renders the answer with its chips; a file chip jumps to the file, a row chip opens the query", async () => {
    installBridge(() => ({
      status: "answered", answer: "Acme renews in March [S1] and ordered 3 times [S2, S3].", uncited: false, cli: 'oo ontology search "Acme"',
      steps: [{ n: 1, tool: "search", arg: "Acme", ok: true }],
      sources: [
        { id: "S1", kind: "file", fileId: "f-1", driveId: "d", path: "contracts/acme.md", anchor: "L3", blockId: "b" },
        { id: "S2", kind: "row", source: "shop", table: "public.orders", pk: "7", keyColumn: "id" },
        { id: "S3", kind: "query", sql: "SELECT count(*) FROM orders", queryId: "q", source: "shop" },
      ],
    }));
    render(<Shell><Ask /></Shell>);
    await ask("When does Acme renew?");
    const chips = await screen.findAllByTestId("ask-source");
    expect(chips.map((c) => c.getAttribute("data-kind"))).toEqual(["file", "row", "query"]);
    expect(within(chips[0]!).getByText("acme.md")).toBeInTheDocument();
    expect(within(chips[0]!).getByText("#L3")).toBeInTheDocument();
    // Inline tags became the numbers of their chips.
    expect(screen.getByTestId("ask-answer-text").textContent).toContain("Acme renews in March 1 and ordered 3 times 2, 3.");
    fireEvent.click(chips[0]!);
    expect(screen.getByTestId("where").dataset.focus).toBe("f-1");
    fireEvent.click(chips[1]!);
    expect(screen.getByTestId("where").dataset).toMatchObject({ section: "ontology", tab: "query" });
    expect(queryDraft.take()).toBe(rowQuery("public.orders", "id", "7"));
    fireEvent.click(chips[2]!);
    expect(queryDraft.take()).toBe("SELECT count(*) FROM orders");
  });

  it("a file source without a file id is a plain chip, not a button", async () => {
    installBridge(() => ({ status: "answered", answer: "x [S1]", sources: [{ id: "S1", kind: "file", fileId: null, driveId: "d", path: "a/b.pdf", anchor: "p.2", blockId: "" }], steps: [] }));
    render(<Shell><Ask /></Shell>);
    await ask("q");
    const chip = await screen.findByTestId("ask-source");
    expect(chip.tagName).toBe("SPAN");
  });

  it("says so when an answer cites nothing, and shows the equivalent command and copy", async () => {
    installBridge(() => ({ status: "answered", answer: "It is 42.", uncited: true, sources: [], steps: [{ n: 1, tool: "search", arg: "x", ok: true }], cli: 'oo ontology search "x"' }));
    render(<Shell><Ask /></Shell>);
    await ask("q");
    expect(await screen.findByTestId("ask-uncited")).toBeInTheDocument();
    expect(screen.queryByTestId("ask-sources")).toBeNull();
    expect(screen.getByTestId("ask-copy")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("ask-cli-toggle"));
    expect(screen.getByTestId("ask-cli-value")).toHaveTextContent('oo ontology search "x"');
  });

  it("splits answer text into paragraphs and keeps text without tags as is", () => {
    render(<TestLocalizationProvider messages={messages}><AnswerText text={"one\ntwo [S9]"} /></TestLocalizationProvider>);
    expect(screen.getAllByText(/one|two/u)).toHaveLength(2);
  });
});

describe("Ask: the unanswerable path", () => {
  it("states plainly that the data has no answer, shows the reason and what was searched, and shows no chips", async () => {
    installBridge(() => ({ status: "unanswerable", answer: "", reason: "No document states a salary.", searched: [{ tool: "search", arg: "salary" }, { tool: "find_text", arg: "薪资" }], cli: 'oo ontology search "salary"' }));
    render(<Shell><Ask /></Shell>);
    await ask("What is the CEO salary?");
    const box = await screen.findByTestId("ask-unanswerable");
    expect(within(box).getByText("Your data doesn't answer this")).toBeInTheDocument();
    expect(screen.getByTestId("ask-reason")).toHaveTextContent("No document states a salary.");
    const searched = within(screen.getByTestId("ask-searched")).getAllByTestId("ask-step");
    expect(searched.map((s) => s.textContent)).toEqual(["Searching “salary”", "Looking for passages with “薪资”"]);
    expect(screen.queryByTestId("ask-source")).toBeNull();
    expect(screen.queryByTestId("ask-copy")).toBeNull();
  });
});

describe("Ask: progress and history", () => {
  it("shows each step quietly while the answer is pending, then the answer", async () => {
    let finish: ((value: AskOutcome) => void) | null = null;
    const bridge = installBridge(() => { const p: Pending = { resolve: () => {} }; Object.defineProperty(p, "resolve", { set: (f) => { finish = f; }, get: () => finish }); return p; });
    render(<Shell><Ask /></Shell>);
    await ask("Who is Evan Lin?");
    const progress = await screen.findByTestId("ask-progress");
    expect(screen.getByTestId("ask-stop")).toBeInTheDocument();
    const askId = (bridge.calls[0] as [string, { askId: string }])[1].askId;
    bridge.push({ askId, type: "step", n: 1, tool: "search", arg: "林远舟", state: "running" });
    expect(within(progress).getByText("Searching “林远舟”")).toBeInTheDocument();
    bridge.push({ askId, type: "step", n: 1, tool: "search", arg: "林远舟", state: "done" });
    bridge.push({ askId, type: "step", n: 2, tool: "entity", arg: "oo:doc/d/person:evan", state: "running" });
    expect(within(progress).getAllByTestId("ask-step").map((s) => s.getAttribute("data-state"))).toEqual(["done", "running"]);
    expect(within(progress).getByText("Reading person:evan")).toBeInTheDocument();
    await act(async () => { finish?.({ status: "answered", answer: "Evan Lin is a PM [S1].", sources: [{ id: "S1", kind: "file", fileId: "f", driveId: "d", path: "team.md", anchor: "", blockId: "" }], steps: [] }); });
    await waitFor(() => expect(screen.getByTestId("ask-answer")).toBeInTheDocument());
    expect(screen.queryByTestId("ask-progress")).toBeNull();
  });

  it("keeps a short history of the session and sends earlier turns with the next question", async () => {
    const bridge = installBridge((r) => ({ status: "answered", answer: `answer to ${r.question}`, sources: [], uncited: true, steps: [] }));
    render(<Shell><Ask /></Shell>);
    await ask("first");
    await screen.findByText("answer to first");
    await ask("second");
    await screen.findByText("answer to second");
    expect(screen.getAllByTestId("ask-turn")).toHaveLength(2);
    expect((bridge.calls[1] as [string, { history: unknown }])[1].history).toEqual([{ question: "first", answer: "answer to first" }]);
    expect(screen.getAllByTestId("ask-question").map((q) => q.textContent)).toEqual(["second", "first"]);
    fireEvent.click(screen.getByTestId("ask-clear"));
    expect(screen.queryAllByTestId("ask-turn")).toHaveLength(0);
  });
});

describe("Ask: calm inline errors with a next step", () => {
  const cases: [AskOutcome["status"], string, string][] = [
    ["no_model", "No model is set up", "Open model settings"],
    ["key_unreadable", "The model key can't be read", "Open model settings"],
    ["model_rejected", "The model refused the key", "Open model settings"],
    ["model_timeout", "The model took too long", "Try again"],
    ["model_failed", "The model didn't answer", "Try again"],
    ["engine_down", "The engine isn't running", "Open engine"],
  ];
  for (const [status, title, action] of cases) {
    it(`${status}: says what happened and offers "${action}"`, async () => {
      installBridge(() => ({ status } as AskOutcome));
      render(<Shell><Ask /></Shell>);
      await ask("q");
      const note = await screen.findByTestId("ask-error");
      expect(within(note).getByText(title)).toBeInTheDocument();
      expect(within(note).getByRole("button", { name: action })).toBeInTheDocument();
    });
  }
  it("the settings action goes to the model tab", async () => {
    installBridge(() => ({ status: "no_model" }));
    render(<Shell><Ask /></Shell>);
    await ask("q");
    fireEvent.click(await screen.findByRole("button", { name: "Open model settings" }));
    expect(screen.getByTestId("where").dataset.section).toBe("settings");
  });
  it("a failed call to the main process is an inline error too", async () => {
    installBridgeReader(() => ({ invoke: async () => ({ ok: false, error: { code: "SO_INTERNAL", retryable: false, details: {} } }), subscribe: () => () => undefined } as never));
    render(<Shell><Ask /></Shell>);
    await ask("q");
    expect(await screen.findByTestId("ask-error")).toBeInTheDocument();
  });
});
