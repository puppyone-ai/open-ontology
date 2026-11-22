// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { SpeakerReviewGroup } from "../../../../src/renderer/screens/ontology/SpeakerReviewGroup";
import type { MediaController } from "../../../../src/renderer/features/media/useMediaController";
import type { MediaSource } from "../../../../src/renderer/features/media/useMediaSource";
import { catalog } from "../../../support/locales";

vi.mock("../../../../src/renderer/features/media/usePersonNames", () => ({ usePersonNames: () => ["Daniel Ortiz", "Priya Raman"] }));

const messages = mergeCatalogNamespaces({ app: catalog("en", "app"), "super-drive": catalog("en", "super-drive") });
afterEach(cleanup);

const item = (id: string, speaker: string, candidates: unknown[]) => ({
  id, kind: "speaker_identity", status: "pending", revision: 3, reason_code: "speaker_unconfirmed", created_at: "2026-11-20T00:00:00Z",
  source: { file_id: "f1", path: "meetings/budget-call.m4a" },
  subject: { b3: "ab".repeat(32), speaker, letter: speaker === "S1" ? "A" : "B", placeholder: `Speaker ${speaker === "S1" ? "A" : "B"} · abababab`, candidates },
}) as never;

const utt = (id: string, speaker: string, start: number, end: number, text: string) => ({ id, startMs: start * 1000, endMs: end * 1000, speaker, text, lowConfidence: false, src: null });
const ready = (utterances = [utt("u1", "S1", 0, 7, "Hello, this is the budget call."), utt("u2", "S2", 8, 16, "Thanks, the numbers are ready."), utt("u3", "S1", 20, 28, "Next is the supplier quote.")]): MediaSource => ({
  state: "ready", fileId: "f1", name: "budget-call.m4a", path: "meetings/budget-call.m4a", kind: "audio", url: "oo-drive://x/budget-call.m4a", utterances, info: { class: "conversation", kind: "conversation", source: "rules", speakersToName: 2 }, reload: () => undefined,
});
const controller = (over: Partial<MediaController> = {}): MediaController => ({
  attach: () => undefined, playing: false, currentMs: 0, durationMs: 30_000, owner: null, failed: false,
  playRange: vi.fn(), playQueue: vi.fn(), seek: vi.fn(), toggle: vi.fn(), pause: vi.fn(), ...over,
});

function show(over: { source?: MediaSource; ctl?: MediaController; active?: boolean; onDecide?: ReturnType<typeof vi.fn>; onActivate?: ReturnType<typeof vi.fn> } = {}) {
  const onDecide = over.onDecide ?? vi.fn(async () => undefined);
  const onActivate = over.onActivate ?? vi.fn();
  const ctl = over.ctl ?? controller();
  render(
    <TestLocalizationProvider messages={messages}>
      <SpeakerReviewGroup items={[item("i1", "S1", [{ name: "Wanqing Shao", source: "self_intro", quote: "I am Wanqing Shao" }]), item("i2", "S2", [])]} fileName="budget-call.m4a" driveName="Docs" busy={null}
        onDecide={onDecide as never} source={over.source ?? ready()} controller={ctl} active={over.active ?? true} onActivate={onActivate} />
    </TestLocalizationProvider>,
  );
  return { onDecide, onActivate, ctl };
}

describe("who is speaking in one recording", () => {
  it("lists Speaker A and Speaker B, says nothing waits, and offers a play button for each", () => {
    show();
    const rows = screen.getAllByTestId("speaker-row");
    expect(rows.map((row) => row.getAttribute("data-speaker"))).toEqual(["S1", "S2"]);
    expect(within(rows[0]).getByText("Speaker A")).toBeInTheDocument();
    expect(within(rows[1]).getByText("Speaker B")).toBeInTheDocument();
    expect(screen.getByText(/Already in the ontology as Speaker A, B/)).toBeInTheDocument();
    expect(screen.getAllByTestId("speaker-play")).toHaveLength(2);
  });

  it("plays a speaker's clips when the speaker's play button is pressed, and shows what each clip says", () => {
    const { ctl } = show();
    const [a] = screen.getAllByTestId("speaker-row");
    expect(within(a).getAllByTestId("speaker-clip-text").map((node) => node.textContent)).toEqual(["Hello, this is the budget call.", "Next is the supplier quote."]);
    fireEvent.click(within(a).getByTestId("speaker-play"));
    expect(ctl.playQueue).toHaveBeenCalledWith([expect.objectContaining({ startMs: 0, endMs: 7000 }), expect.objectContaining({ startMs: 20_000, endMs: 28_000 })], "S1");
    fireEvent.click(within(a).getAllByTestId("speaker-clip-play")[1]);
    expect(ctl.playRange).toHaveBeenCalledWith({ startMs: 20_000, endMs: 28_000 }, "S1");
    fireEvent.click(within(a).getAllByTestId("speaker-open-at")[0]);
    expect(ctl.seek).toHaveBeenCalledWith(0, { owner: "S1" });
  });

  it("pauses when the speaker that is playing is pressed again, and marks the clip being heard", () => {
    const ctl = controller({ playing: true, owner: "S1", currentMs: 21_000 });
    show({ ctl });
    const [a] = screen.getAllByTestId("speaker-row");
    expect(a).toHaveAttribute("data-playing", "true");
    expect(within(a).getAllByTestId("speaker-clip").map((clip) => clip.getAttribute("data-now"))).toEqual([null, "true"]);
    fireEvent.click(within(a).getByTestId("speaker-play"));
    expect(ctl.pause).toHaveBeenCalled();
  });

  it("asks for the recording to be shown first when it is not the one in the pane", () => {
    const { onActivate } = show({ source: { state: "loading" }, active: false });
    expect(screen.queryAllByTestId("speaker-clip")).toHaveLength(0);
    fireEvent.click(screen.getAllByTestId("speaker-play")[1]);
    expect(onActivate).toHaveBeenCalledWith("S2");
  });

  it("names a speaker from a candidate, a typed or picked name, and keeps 'not anyone known' for the rest", () => {
    const { onDecide } = show();
    const [a, b] = screen.getAllByTestId("speaker-row");
    fireEvent.click(within(a).getByTestId("speaker-confirm-candidate"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i1" }), "accept", { name: "Wanqing Shao" });
    expect(within(b).getByTestId("speaker-confirm-typed")).toBeDisabled();
    expect(Array.from(document.querySelectorAll("datalist option")).map((option) => option.getAttribute("value"))).toContain("Priya Raman");
    fireEvent.change(within(b).getByTestId("speaker-name"), { target: { value: "  Daniel Ortiz " } });
    fireEvent.click(within(b).getByTestId("speaker-confirm-typed"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i2" }), "accept", { name: "Daniel Ortiz" });
    fireEvent.change(within(b).getByTestId("speaker-name"), { target: { value: "Priya Raman" } });
    fireEvent.keyDown(within(b).getByTestId("speaker-name"), { key: "Enter" });
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i2" }), "accept", { name: "Priya Raman" });
    fireEvent.click(within(b).getByTestId("speaker-none"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i2" }), "reject");
  });

  it("says so when a voice has no clear stretch to play", () => {
    show({ source: ready([utt("u1", "S1", 0, 0.3, "hm")]) });
    expect(screen.getAllByTestId("speaker-no-clips").length).toBeGreaterThan(0);
  });
});
