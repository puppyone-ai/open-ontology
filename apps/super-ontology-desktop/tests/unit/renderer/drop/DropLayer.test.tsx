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
import { emptySkipped, initialUploadQueueState, type BatchInfo, summarizeDrop, type DropOutcome, type UploadQueueState } from "../../../../src/renderer/model/queue/uploadQueue";
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

const outcome = (over: Partial<DropOutcome> = {}): DropOutcome => ({ batchIds: ["b1"], error: null, ...over });
const batch = (over: Partial<BatchInfo> = {}): BatchInfo => ({
  id: "b1", driveId: "d-my", folder: "", phase: "done", found: 3, bytes: 3000, queued: 3, counts: { done: 3, failed: 0, skipped: 0, cancelled: 0 }, skipped: emptySkipped(),
  samples: [], roots: [], large: [], emptyFolders: 0, problem: null, ...over,
});
const setBatches = (...batches: BatchInfo[]) => { queueState.current = { ...initialUploadQueueState(), batches }; };
const drives = [{ id: "d-my", name: "My files", backend: "fs" }, { id: "d-team", name: "Team docs", backend: "fs" }, { id: "d-s3", name: "Archive", backend: "s3" }] as DropLayerDeps["drives"];
function deps(over: Partial<DropLayerDeps> = {}): DropLayerDeps {
  return {
    drives, firstRunOpen: false, ensureMyFiles: vi.fn(async () => ({ ok: true as const, value: { id: "d-my" } })), enqueue: vi.fn(async () => outcome()), keepInSync: vi.fn(async () => ({ ok: true as const })),
    decide: vi.fn(async () => true), cancelBatches: vi.fn(async () => undefined), ...over,
  };
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
  const flush = async () => { await act(async () => { await Promise.resolve(); await Promise.resolve(); }); };

  it("copies the files out of the drop and sends them to My files, creating it when it is not listed yet", async () => {
    setBatches(batch());
    const d = deps({ drives: [] });
    render(wrap(<DropLayer {...d} />));
    const files = [file("a.md"), file("Folder")];
    const event = dragEvent("drop", document.body, { files });
    expect(event.defaultPrevented).toBe(true);
    await flush();
    expect(d.ensureMyFiles).toHaveBeenCalledTimes(1);
    expect(d.enqueue).toHaveBeenCalledWith("d-my", "", files);
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-queued", "3");
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("This drop: 3 files added to “My files”");
  });

  it("answers in the frame of the drop: Preparing… before anything is awaited, and the app is asked to show the Sources screen", async () => {
    let release: (value: DropOutcome) => void = () => undefined;
    const onDrop = vi.fn();
    const d = deps({ onDrop, enqueue: vi.fn(() => new Promise<DropOutcome>((resolve) => { release = resolve; })) });
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Project")] });
    // No await: the engine has not answered, the walk has not started.
    expect(onDrop).toHaveBeenCalledTimes(1);
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "preparing");
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Preparing…");
    await flush();
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "preparing");
    await act(async () => { release(outcome()); await Promise.resolve(); });
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "preparing"); // the batch is not reported yet
  });

  it("shows the files found so far while the walk goes on, then what was added", async () => {
    setBatches(batch({ phase: "scanning", found: 0, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 } }));
    const d = deps();
    const { rerender } = render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Project")] });
    await flush();
    setBatches(batch({ phase: "scanning", found: 4210, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 } }));
    rerender(wrap(<DropLayer {...d} />));
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "preparing");
    expect(screen.getByTestId("drop-toast-found")).toHaveTextContent("4,210 files found so far");
    setBatches(batch({ phase: "copying", found: 4300, queued: 4300, counts: { done: 120, failed: 0, skipped: 0, cancelled: 0 } }));
    rerender(wrap(<DropLayer {...d} />));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Adding files: 120 of 4,300");
    setBatches(batch({ phase: "done", found: 4300, queued: 4300, counts: { done: 4300, failed: 0, skipped: 0, cancelled: 0 } }));
    rerender(wrap(<DropLayer {...d} />));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("This drop: 4,300 files added");
  });

  it("sends a drop on a source row to that source and a drop in its folder into the folder", async () => {
    setBatches(batch({ driveId: "d-team" }));
    const d = deps();
    render(wrap(<><section data-testid="detail" {...dropAttributes(drives[1], "sub/dir")}><b>list</b></section><DropLayer {...d} /></>));
    dragEvent("drop", screen.getByText("list"), { files: [file("n.md")] });
    await flush();
    expect(d.ensureMyFiles).not.toHaveBeenCalled();
    expect(d.enqueue).toHaveBeenCalledWith("d-team", "sub/dir", expect.any(Array));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("This drop: 3 files added to “Team docs”");
  });

  it("copies nothing onto a cloud source and says nothing was queued", async () => {
    const d = deps();
    render(wrap(<><div data-testid="cloud" {...dropAttributes(drives[2])}>Archive</div><DropLayer {...d} /></>));
    dragEvent("drop", screen.getByTestId("cloud"), { files: [file("n.md")] });
    await flush();
    expect(d.enqueue).not.toHaveBeenCalled();
    expect(screen.queryByTestId("drop-toast")).toBeNull();
  });

  it("answers a drop that could not be started with the sentence for its error", async () => {
    const d = deps({ enqueue: vi.fn(async () => outcome({ batchIds: [], error: { code: "SO_UPLOAD_UNSUPPORTED_BACKEND", retryable: false, details: {} } })) });
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Huge")] });
    await flush();
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent(/.+/u);
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "failed");
  });

  it("offers to keep a small dropped folder in sync, defaulting to the copy, and registers it on a click", async () => {
    setBatches(batch({ roots: [{ path: "/Users/me/ProjectA", name: "ProjectA", kind: "directory", files: 3 }, { path: "/Users/me/n.md", name: "n.md", kind: "file", files: 1 }] }));
    const d = deps();
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("ProjectA"), file("n.md")] });
    await flush();
    expect(d.keepInSync).not.toHaveBeenCalled();
    const offer = screen.getByTestId("drop-keep-in-sync");
    expect(offer).toHaveTextContent("Also keep this folder in sync");
    fireEvent.click(offer);
    await flush();
    expect(d.keepInSync).toHaveBeenCalledWith({ path: "/Users/me/ProjectA", name: "ProjectA" }, ["b1"]);
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-sync", "done");
    expect(screen.getByTestId("drop-toast-synced")).toHaveTextContent("in place of the copy.");
    expect(screen.queryByTestId("drop-keep-in-sync")).toBeNull();
  });

  it("offers nothing for loose files, and keeps the offer open for a retry when registering fails", async () => {
    setBatches(batch({ roots: [{ path: "/a/n.md", name: "n.md", kind: "file", files: 1 }] }));
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("drop", document.body, { files: [file("n.md")] });
    await flush();
    expect(screen.queryByTestId("drop-keep-in-sync")).toBeNull();
    cleanup();
    setBatches(batch({ roots: [{ path: "/a/P", name: "P", kind: "directory", files: 1 }] }));
    const failing = deps({ keepInSync: vi.fn(async () => ({ ok: false as const, error: { code: "SO_UPLOAD_SOURCE_MISSING", retryable: false, details: {} } })) });
    render(wrap(<DropLayer {...failing} />));
    dragEvent("drop", document.body, { files: [file("P")] });
    await flush();
    fireEvent.click(screen.getByTestId("drop-keep-in-sync"));
    await flush();
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-sync", "failed");
    expect(screen.getByTestId("drop-keep-in-sync")).toBeInTheDocument();
  });
});

describe("a big folder", () => {
  const file = (name: string) => new File(["x"], name);
  const flush = async () => { await act(async () => { await Promise.resolve(); await Promise.resolve(); }); };
  const large = { path: "/Users/me/Big", name: "Big", files: 25_001, bytes: 2.5 * 1024 ** 3, state: "pending" as const };
  const asking = () => batch({ phase: "choice", found: 25_001, bytes: 2.5 * 1024 ** 3, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 }, large: [large], roots: [{ path: large.path, name: "Big", kind: "directory", files: 25_001 }], skipped: { ...emptySkipped(), junk: 7, gitignore: 3 } });

  it("asks before copying, with linking as the recommended primary action and copying as the secondary", async () => {
    setBatches(asking());
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-phase", "choice");
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("“Big” is a big folder: 25,001 files, 2.5 GB");
    expect(screen.getByTestId("drop-toast-skipped")).toHaveTextContent("10 items skipped");
    const link = screen.getByTestId("drop-link-folder");
    expect(link).toHaveTextContent("Link this folder (recommended for big projects: no copy, stays in sync)");
    expect(link).toHaveAttribute("data-variant", "primary");
    expect(screen.getByTestId("drop-copy-folder")).toHaveTextContent("Copy into My files");
    expect(screen.getByTestId("drop-copy-folder")).toHaveAttribute("data-variant", "secondary");
  });

  it("links the folder as a source with nothing copied, and tells the main process to leave it", async () => {
    setBatches(asking());
    const d = deps();
    const { rerender } = render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    fireEvent.click(screen.getByTestId("drop-link-folder"));
    await flush();
    expect(d.keepInSync).toHaveBeenCalledWith({ path: "/Users/me/Big", name: "Big" }, []);
    expect(d.decide).toHaveBeenCalledWith("b1", "skip", ["/Users/me/Big"]);
    setBatches({ ...asking(), phase: "done", large: [{ ...large, state: "skipped" }] });
    rerender(wrap(<DropLayer {...d} />));
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-choice", "linked");
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Linked “Big”: 25,001 files");
    expect(screen.getByTestId("drop-toast-linked")).toHaveTextContent("Nothing was copied");
  });

  it("copies it as a stream when asked to", async () => {
    setBatches(asking());
    const d = deps();
    const { rerender } = render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    fireEvent.click(screen.getByTestId("drop-copy-folder"));
    await flush();
    expect(d.decide).toHaveBeenCalledWith("b1", "copy");
    expect(d.keepInSync).not.toHaveBeenCalled();
    setBatches({ ...asking(), phase: "copying", queued: 600, counts: { done: 100, failed: 0, skipped: 0, cancelled: 0 }, large: [{ ...large, state: "copying" }] });
    rerender(wrap(<DropLayer {...d} />));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Adding files: 100 of 600");
  });

  it("keeps the choice open for a retry when linking fails", async () => {
    setBatches(asking());
    const d = deps({ keepInSync: vi.fn(async () => ({ ok: false as const, error: { code: "SO_UPLOAD_SOURCE_MISSING", retryable: false, details: {} } })) });
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    fireEvent.click(screen.getByTestId("drop-link-folder"));
    await flush();
    expect(d.decide).not.toHaveBeenCalled();
    expect(screen.getByTestId("drop-toast")).toHaveAttribute("data-sync", "failed");
    expect(screen.getByTestId("drop-link-folder")).toBeInTheDocument();
  });

  it("stays while the choice is open, and cancelling stops the drop", async () => {
    vi.useFakeTimers();
    setBatches(asking());
    const d = deps();
    render(wrap(<DropLayer {...d} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(screen.getByTestId("drop-toast")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("drop-toast-cancel"));
    expect(d.cancelBatches).toHaveBeenCalledWith(["b1"]);
    expect(screen.queryByTestId("drop-toast")).toBeNull();
  });

  it("says plainly when the disk cannot hold the drop, with the numbers, and what to do", async () => {
    setBatches(batch({ phase: "done", found: 5, queued: 5, counts: { done: 0, failed: 5, skipped: 0, cancelled: 0 }, problem: { code: "SO_UPLOAD_NOT_ENOUGH_SPACE", needed: 3 * 1024 ** 3, free: 1024 ** 3 } }));
    render(wrap(<DropLayer {...deps()} />));
    dragEvent("drop", document.body, { files: [file("Big")] });
    await flush();
    const problem = screen.getByTestId("drop-toast-problem");
    expect(problem).toHaveTextContent("Not enough free disk space for this drop. Free up space, or link the folder instead of copying it.");
    expect(problem).toHaveTextContent("needs about 3 GB; 1 GB is free.");
  });
});

describe("the toast", () => {
  const state = (over: Partial<DropToastState> = {}): DropToastState => ({ id: 1, name: "Team docs", managed: false, started: true, batchIds: ["b1"], error: null, sync: "idle", syncError: null, choice: "none", linked: [], ...over });
  const none = () => undefined;
  const summary = (over: Partial<BatchInfo> = {}) => summarizeDrop([batch(over)], ["b1"]);
  const renderToast = (toast: DropToastState, sum: ReturnType<typeof summary>, onClose: () => void = none) => render(wrap(<DropToast toast={toast} summary={sum} onSync={none} onLink={none} onCopy={none} onCancel={none} onClose={onClose} />));

  it("says how many items were skipped", () => {
    renderToast(state(), summary({ skipped: { ...emptySkipped(), hidden: 2, junk: 1 } }));
    expect(screen.getByTestId("drop-toast-skipped")).toHaveTextContent("3 items skipped");
  });
  it("says so when everything was skipped", () => {
    renderToast(state(), summary({ found: 0, queued: 0, counts: { done: 0, failed: 0, skipped: 0, cancelled: 0 }, skipped: { ...emptySkipped(), hidden: 2 } }));
    expect(screen.getByTestId("drop-toast-title")).toHaveTextContent("Nothing to add");
  });
  it("leaves by itself, but not while it is being read", () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    renderToast(state(), summary(), onClose);
    fireEvent.mouseEnter(screen.getByTestId("drop-toast"));
    act(() => { vi.advanceTimersByTime(30_000); });
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.mouseLeave(screen.getByTestId("drop-toast"));
    act(() => { vi.advanceTimersByTime(8000); });
    expect(onClose).toHaveBeenCalledTimes(1);
  });
  it("does not leave while it is still preparing or copying", () => {
    vi.useFakeTimers();
    const onClose = vi.fn();
    renderToast(state(), summary({ phase: "scanning", found: 10 }), onClose);
    act(() => { vi.advanceTimersByTime(60_000); });
    expect(onClose).not.toHaveBeenCalled();
  });
});
