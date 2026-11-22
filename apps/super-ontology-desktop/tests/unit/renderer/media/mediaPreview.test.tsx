// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { MediaPreview } from "../../../../src/renderer/features/media/MediaPreview";
import { useMediaController } from "../../../../src/renderer/features/media/useMediaController";
import type { MediaSource } from "../../../../src/renderer/features/media/useMediaSource";
import { catalog } from "../../../support/locales";

const messages = mergeCatalogNamespaces({ app: catalog("en", "app"), "super-drive": catalog("en", "super-drive") });
const utt = (id: string, speaker: string, start: number, end: number, text: string) => ({ id, startMs: start * 1000, endMs: end * 1000, speaker, text, lowConfidence: false, src: null });
const source = (kind: "audio" | "video" = "audio"): MediaSource => ({
  state: "ready", fileId: "f1", name: "call.m4a", path: "m/call.m4a", kind, url: "oo-drive://t/call.m4a",
  utterances: [utt("u1", "S1", 0, 5, "Good morning."), utt("u2", "S2", 6, 12, "Morning, shall we start?"), utt("u3", "S1", 14, 20, "Yes, the first item.")],
  info: { class: "conversation", kind: "conversation", source: "rules", speakersToName: 2 }, reload: () => undefined,
});

let played = 0;
beforeEach(() => {
  played = 0;
  Object.defineProperty(HTMLMediaElement.prototype, "play", { configurable: true, value() { played += 1; this.dispatchEvent(new Event("play")); return Promise.resolve(); } });
  Object.defineProperty(HTMLMediaElement.prototype, "pause", { configurable: true, value() { this.dispatchEvent(new Event("pause")); } });
  Object.defineProperty(HTMLMediaElement.prototype, "paused", { configurable: true, get() { return !this.dataset.playing; } });
  vi.stubGlobal("requestAnimationFrame", () => 0);
  vi.stubGlobal("cancelAnimationFrame", () => undefined);
});
afterEach(() => { cleanup(); vi.unstubAllGlobals(); });

function Harness({ src, startMs, autoplay }: { src: MediaSource; startMs?: number; autoplay?: boolean }) {
  const controller = useMediaController();
  return <TestLocalizationProvider messages={messages}><MediaPreview source={src} controller={controller} startMs={startMs} autoplay={autoplay} speakerText={(label) => `Speaker ${label.slice(1)}`} /></TestLocalizationProvider>;
}

describe("the media preview", () => {
  it("shows an audio player with the transcript and a native audio element on the capability URL", () => {
    render(<Harness src={source()} />);
    expect(screen.getByTestId("media-element").tagName).toBe("AUDIO");
    expect(screen.getByTestId("media-element")).toHaveAttribute("src", "oo-drive://t/call.m4a");
    expect(screen.getAllByTestId("media-utterance")).toHaveLength(3);
    expect(screen.getByText("Morning, shall we start?")).toBeInTheDocument();
  });

  it("uses a video element for a video", () => {
    render(<Harness src={source("video")} />);
    expect(screen.getByTestId("media-element").tagName).toBe("VIDEO");
  });

  it("plays from a clicked line and marks the line being said as the player moves", () => {
    render(<Harness src={source()} />);
    const element = screen.getByTestId("media-element") as HTMLMediaElement;
    fireEvent.click(screen.getAllByTestId("media-utterance")[1]);
    expect(element.currentTime).toBe(6);
    expect(played).toBe(1);
    act(() => { element.currentTime = 15; element.dispatchEvent(new Event("timeupdate")); });
    const rows = screen.getAllByTestId("media-utterance");
    expect(rows.map((row) => row.getAttribute("aria-current"))).toEqual([null, null, "true"]);
    expect(screen.getByTestId("media-time")).toHaveTextContent("0:15");
  });

  it("moves to the second an evidence mark points at, and plays when asked to", () => {
    render(<Harness src={source()} startMs={14_000} autoplay />);
    expect((screen.getByTestId("media-element") as HTMLMediaElement).currentTime).toBe(14);
    expect(played).toBe(1);
  });

  it("says it cannot play an original in object storage, and shows a skeleton while loading", () => {
    const { rerender } = render(<Harness src={{ state: "unavailable", reason: "s3" }} />);
    expect(screen.getByTestId("media-unavailable")).toHaveTextContent("object storage");
    rerender(<Harness src={{ state: "loading" }} />);
    expect(screen.queryByTestId("media-pane")).toBeNull();
  });
});
