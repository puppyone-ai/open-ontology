// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { SpeakerReviewRow } from "../../../../src/renderer/screens/ontology/SpeakerReviewRow";
import { catalog } from "../../../support/locales";

const messages = mergeCatalogNamespaces({ app: catalog("en", "app"), "super-drive": catalog("en", "super-drive") });
afterEach(cleanup);

const item = (candidates: unknown[]) => ({
  id: "i1", kind: "speaker_identity", status: "pending", revision: 3, reason_code: "speaker_unconfirmed", created_at: "2026-11-20T00:00:00Z",
  source: { file_id: "f1", path: "meetings/budget-call.m4a" },
  subject: { b3: "ab".repeat(32), speaker: "S1", placeholder: "Speaker 1 · abababab", candidates },
}) as never;

function show(candidates: unknown[], onDecide = vi.fn(async () => undefined)) {
  render(<TestLocalizationProvider messages={messages}><SpeakerReviewRow item={item(candidates)} drive="Docs" busy={null} onDecide={onDecide} /></TestLocalizationProvider>);
  return onDecide;
}

describe("who is Speaker 1", () => {
  it("says why it is here and offers each candidate name, the first being a self-introduction", () => {
    show([{ name: "Wanqing Shao", source: "self_intro", quote: "I am Wanqing Shao", block_id: "b1" }, { name: "Ruobing Bai", source: "attendees" }]);
    expect(screen.getByText("Speaker 1: who is this?")).toBeInTheDocument();
    expect(screen.getByText(/Not confirmed yet/)).toBeInTheDocument();
    expect(screen.getByText("I am Wanqing Shao")).toBeInTheDocument();
    expect(screen.getAllByTestId("speaker-confirm-candidate").map((b) => b.textContent)).toEqual(["Confirm as Wanqing Shao", "Confirm as Ruobing Bai"]);
  });
  it("confirms a candidate, a typed name, or keeps the speaker unknown", () => {
    const onDecide = show([{ name: "Wanqing Shao", source: "self_intro" }]);
    fireEvent.click(screen.getByTestId("speaker-confirm-candidate"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i1" }), "accept", { name: "Wanqing Shao" });
    expect(screen.getByTestId("speaker-confirm-typed")).toBeDisabled();
    fireEvent.change(screen.getByTestId("speaker-name"), { target: { value: "  Daniel Ortiz " } });
    fireEvent.click(screen.getByTestId("speaker-confirm-typed"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i1" }), "accept", { name: "Daniel Ortiz" });
    fireEvent.click(screen.getByTestId("speaker-none"));
    expect(onDecide).toHaveBeenLastCalledWith(expect.objectContaining({ id: "i1" }), "reject");
  });
  it("with no candidate it still lets a person type a name", () => {
    show([]);
    expect(screen.getByText("No candidate names were found in this recording.")).toBeInTheDocument();
    expect(screen.queryAllByTestId("speaker-confirm-candidate")).toHaveLength(0);
  });
});
