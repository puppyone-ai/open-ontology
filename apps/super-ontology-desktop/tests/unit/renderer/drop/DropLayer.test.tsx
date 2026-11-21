// @vitest-environment jsdom
import "@testing-library/jest-dom/vitest";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TestLocalizationProvider } from "@open-ontology/design-system/localization/testing";
import { mergeCatalogNamespaces } from "@open-ontology/design-system/localization/core";
import { DropLayer, type DropLayerDeps } from "../../../../src/renderer/features/drop/DropLayer";
import { DropOverlay } from "../../../../src/renderer/features/drop/DropOverlay";
import { DropToast, type DropToastState } from "../../../../src/renderer/features/drop/DropToast";
import { databaseDropAttributes, dropAttributes } from "../../../../src/renderer/features/drop/dropTarget";
import { emptySkipped, initialUploadQueueState, type DropOutcome, type UploadQueueState } from "../../../../src/renderer/model/queue/uploadQueue";
import { catalog } from "../../../support/locales";

const queueState: { current: UploadQueueState } = { current: initialUploadQueueState() };
vi.mock("../../../../src/renderer/model/uploadStore", () => ({
  getUploadQueue: () => ({ dismiss: vi.fn(), cancel: vi.fn(), retry: vi.fn() }),
  useUploadQueueState: () => queueState.current,
}));

const messages = (locale: "en" | "zh-Hans" = "en") => mergeCatalogNamespaces({ app: catalog(locale, "app"), "super-drive": catalog(locale, "super-drive") });
const wrap = (node: React.ReactNode, locale: "en" | "zh-Hans" = "en") => <TestLocalizationProvider locale={locale} messages={messages(locale)}>{node}</TestLocalizationProvider>;

function dragEvent(type: string, target: Element | Window, { types = ["Files"], kinds = ["file"], files = [] as File[] } = {}) {
  const event = new Event(type, { bubbles: true, cancelable: true });
  Object.defineProperty(event, "dataTransfer", { value: { types, items: kinds.map((kind) => ({ kind })), files, dropEffect: "none" } });
  act(() => { (target as EventTarget).dispatchEvent(event); });
  return event as Event & { dataTransfer: { dropEffect: string } };
}

const outcome = (over: Partial<DropOutcome> = {}): DropOutcome => ({ queued: 3, skipped: emptySkipped(), roots: [], emptyFolders: 0, error: null, ...over });
const drives = [{ id: "d-my", name: "My files", backend: "fs" }, { id: "d-team", name: "Team docs", backend: "fs" }, { id: "d-s3", name: "Archive", backend: "s3" }] as DropLayerDeps["drives"];
function deps(over: Partial<DropLayerDeps> = {}): DropLayerDeps {
  return { drives, firstRunOpen: false, ensureMyFiles: vi.fn(async () => ({ ok: true as const, value: { id: "d-my" } })), enqueue: vi.fn(async () => outcome()), keepInSync: vi.fn(async () => ({ ok: true as const })), ...over };
}

beforeEach(() => { queueState.current = initialUploadQueueState(); });
afterEach(() => { cleanup(); vi.useRealTimers(); });

describe("the overlay states", () => {
  it("is not there until files are dragged over the window", () => {
    render(wrap(<DropLayer {...deps()} />));
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
    dragEvent("dragenter", document.body, { types: ["text/plain"], kinds: ["string"] });
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
  });

  it("shows the calm overlay for My files with the count of items, in the language of the app", () => {
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("dragenter", document.body, { kinds: ["file", "file", "file"] });
    const overlay = screen.getByTestId("drop-overlay");
    expect(overlay).toHaveAttribute("data-state", "ready");
    expect(overlay).toHaveAttribute("data-target", "my-files");
    expect(screen.getByTestId("drop-overlay-title")).toHaveTextContent("Drop to add to My files");
    expect(screen.getByTestId("drop-overlay-count")).toHaveTextContent("3 items");
    cleanup();
    render(wrap(<DropLayer {...deps()} />, "zh-Hans"));
    dragEvent("dragenter", document.body, { kinds: ["file", "file"] });
    expect(screen.getByTestId("drop-overlay-title")).toHaveTextContent("松开以添加到『我的文件』");
    expect(screen.getByTestId("drop-overlay-count")).toHaveTextContent("2 项");
  });

  it("names the source when the pointer is over its row, and marks the row", () => {
    render(wrap(<><button type="button" data-testid="row" {...dropAttributes(drives[1])}><span>Team docs</span></button><DropLayer {...deps()} /></>));
    const row = screen.getByTestId("row");
    dragEvent("dragenter", row.querySelector("span")!);
    expect(screen.getByTestId("drop-overlay")).toHaveAttribute("data-target", "source");
    expect(screen.getByTestId("drop-overlay-title")).toHaveTextContent("Drop to add to “Team docs”");
    expect(row).toHaveAttribute("data-drop-over");
    dragEvent("dragover", document.body);
    expect(row).not.toHaveAttribute("data-drop-over");
    expect(screen.getByTestId("drop-overlay")).toHaveAttribute("data-target", "my-files");
  });

  it("explains why cloud storage and databases cannot take files, and shows the drop as not allowed", () => {
    render(wrap(<><div data-testid="cloud" {...dropAttributes(drives[2])}>Archive</div><div data-testid="db" {...databaseDropAttributes("shop")}>shop</div><DropLayer {...deps()} /></>));
    dragEvent("dragenter", screen.getByTestId("cloud"));
    expect(screen.getByTestId("drop-overlay")).toHaveAttribute("data-state", "rejected");
    expect(screen.getByTestId("drop-overlay")).toHaveTextContent("Can’t add files to “Archive”");
    expect(screen.getByTestId("drop-overlay")).toHaveTextContent("Cloud storage sources are read-only.");
    expect(screen.queryByTestId("drop-overlay-count")).toBeNull();
    const over = dragEvent("dragover", screen.getByTestId("db"));
    expect(over.dataTransfer.dropEffect).toBe("none");
    expect(screen.getByTestId("drop-overlay")).toHaveTextContent("Database sources don’t take files.");
    expect(dragEvent("dragover", document.body).dataTransfer.dropEffect).toBe("copy");
  });

  it("goes away when the drag leaves the window, and when the drag is cancelled", () => {
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("dragenter", document.body);
    dragEvent("dragenter", document.body);
    dragEvent("dragleave", document.body);
    expect(screen.getByTestId("drop-overlay")).toBeInTheDocument();
    dragEvent("dragleave", document.body);
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
    dragEvent("dragenter", document.body);
    act(() => { window.dispatchEvent(new Event("dragend")); });
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
  });

  it("goes away by itself when no dragover arrives for a while (a lost dragleave)", () => {
    vi.useFakeTimers();
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("dragenter", document.body);
    expect(screen.getByTestId("drop-overlay")).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(2000); });
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
  });

  it("stays out of the way during the first run, but still keeps the window from navigating to a dropped file", () => {
    render(wrap(<DropLayer {...deps({ firstRunOpen: true })} />));
    expect(dragEvent("dragenter", document.body).defaultPrevented).toBe(true);
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
    const dropped = dragEvent("drop", document.body, { files: [new File(["x"], "a.md")] });
    expect(dropped.defaultPrevented).toBe(true);
  });

  it("renders every state in isolation", () => {
    const { rerender } = render(wrap(<DropOverlay over={null} />));
    expect(screen.queryByTestId("drop-overlay")).toBeNull();
    rerender(wrap(<DropOverlay over={{ target: { kind: "drive", driveId: "d", folder: "", name: "Team docs", managed: false }, count: 1 }} />));
    expect(screen.getByTestId("drop-overlay-count")).toHaveTextContent("1 item");
    expect(screen.getByTestId("drop-overlay").getAttribute("role")).toBe("status");
  });
});

describe("dropping", () => {
  const file = (name: string) => new File(["x"], name);

  it("copies the files out of the drop and sends them to My files, creating it when it is not listed yet", async () => {
    const d = deps({ drives: [] });
    render(wrap(<DropLayer {...d} />));
    const files = [file("a.md"), file("Folder")];
    const event = dragEvent("drop", document.body, { files });
    expect(event.defaultPrevented).toBe(true);
    await act(async () => { await Promise.resolve(); });
    expect(d.ensureMyFiles).toHaveBeenCalledTimes(1);
    expect(d.enqueue).toHaveBeenCalledWith("d-my", "", files);
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-queued", "3");
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Added 3 files to “My files”");
  });

  it("sends a drop on a source row to that source and a drop in its folder into the folder", async () => {
    const d = deps();
    render(wrap(<><section data-testid="detail" {...dropAttributes(drives[1], "sub/dir")}><b>list</b></section><DropLayer {...d} /></>));
    dragEvent("drop", screen.getByText("list"), { files: [file("n.md")] });
    await act(async () => { await Promise.resolve(); });
    expect(d.ensureMyFiles).not.toHaveBeenCalled();
    expect(d.enqueue).toHaveBeenCalledWith("d-team", "sub/dir", expect.any(Array));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Added 3 files to “Team docs”");
  });

  it("copies nothing onto a cloud source and says nothing was queued", async () => {
    const d = deps();
    render(wrap(<><div data-testid="cloud" {...dropAttributes(drives[2])}>Archive</div><DropLayer {...d} /></>));
    dragEvent("drop", screen.getByTestId("cloud"), { files: [file("n.md")] });
    await act(async () => { await Promise.resolve(); });
    expect(d.enqueue).not.toHaveBeenCalled();
    expect(screen.queryByTestId("drop-toast")).toBeNull();
  });

  it("answers a failed drop with the sentence for its error", async () => {
    const d = deps({ enqueue: vi.fn(async () => outcome({ queued: 0, error: { code: "SO_UPLOAD_TOO_MANY", retryable: false, details: {} } })) });
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Huge")] });
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Too many files in one drop");
  });

  it("offers to keep a dropped folder in sync, defaulting to the copy, and registers it on a click", async () => {
    const d = deps({ enqueue: vi.fn(async () => outcome({ roots: [{ path: "/Users/me/ProjectA", name: "ProjectA", kind: "directory", files: 3 }, { path: "/Users/me/n.md", name: "n.md", kind: "file", files: 1 }] })) });
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("ProjectA"), file("n.md")] });
    await act(async () => { await Promise.resolve(); });
    expect(d.keepInSync).not.toHaveBeenCalled();
    const offer = screen.getByTestId("drop-keep-in-sync");
    expect(offer).toHaveTextContent("Also keep this folder in sync");
    fireEvent.click(offer);
    await act(async () => { await Promise.resolve(); });
    expect(d.keepInSync).toHaveBeenCalledWith({ path: "/Users/me/ProjectA", name: "ProjectA" });
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-sync", "done");
    expect(screen.getByTestId("drop-toast-synced")).toHaveTextContent("The copy in My files stays.");
    expect(screen.queryByTestId("drop-keep-in-sync")).toBeNull();
  });

  it("offers nothing for loose files, and keeps the offer open for a retry when registering fails", async () => {
    const loose = deps({ enqueue: vi.fn(async () => outcome({ roots: [{ path: "/a/n.md", name: "n.md", kind: "file", files: 1 }] })) });
    render(wrap(<DropLayer {...loose} />));
    dragEvent("drop", document.body, { files: [file("n.md")] });
    await act(async () => { await Promise.resolve(); });
    expect(screen.queryByTestId("drop-keep-in-sync")).toBeNull();
    cleanup();
    const failing = deps({ enqueue: vi.fn(async () => outcome({ roots: [{ path: "/a/P", name: "P", kind: "directory", files: 1 }] })), keepInSync: vi.fn(async () => ({ ok: false as const, error: { code: "SO_UPLOAD_SOURCE_MISSING", retryable: false, details: {} } })) });
    render(wrap(<DropLayer {...failing} />));
    dragEvent("drop", document.body, { files: [file("P")] });
    await act(async () => { await Promise.resolve(); });
    fireEvent.click(screen.getByTestId("drop-keep-in-sync"));
    await act(async () => { await Promise.resolve(); });
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-sync", "failed");
    expect(screen.getByTestId("drop-keep-in-sync")).toBeInTheDocument();
  });
});

describe("the toast", () => {
  const state = (over: Partial<DropToastState> = {}): DropToastState => ({ id: 1, name: "Team docs", managed: false, outcome: outcome(), sync: "idle", syncError: null, ...over });
  it("says how many items were skipped", () => {
    render(wrap(<DropToast toast={state({ outcome: outcome({ skipped: { ...emptySkipped(), hidden: 2, junk: 1 } }) })} onSync={() => undefined} onClose={() => undefined} />));
    expect(screen.getByTestId("drop-toast-skipped")).toHaveTextContent("3 items skipped");
  });
  it("says so when everything was skipped", () => {
    render(wrap(<DropToast toast={state({ outcome: outcome({ queued: 0, skipped: { ...emptySkipped(), hidden: 2 } }) })} onSync={() => undefined} onClose={() => undefined} />));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Nothing to add");
  });
  it("leaves by itself, but not while it is being read", () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    render(wrap(<DropToast toast={state()} onSync={() => undefined} onClose={onClose} />));
    fireEvent.mouseEnter(screen.getByTestId("drop-toast"));
    act(() => { vi.advanceTimersByTime(30_000); });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.mouseLeave(screen.getByTestId("drop-toast"));
    act(() => { vi.advanceTimersByTime(8000); });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
});
