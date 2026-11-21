import { useEffect, useMemo, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { IconButton, ProgressBar, StatusDot } from "@open-ontology/design-system";
import { ChevronDown, ChevronUp, File as FileIcon, RotateCw, X } from "lucide-react";
import { useErrorText } from "../../hooks/useText";
import { getUploadQueue, useUploadQueueState } from "../../model/uploadStore";
import { isUploadActive, type SkippedCounts, type UploadItem } from "../../model/queue/uploadQueue";

/** Rows shown at most; a drop of thousands of files is a summary and the work in flight, not thousands of rows. */
const ROW_LIMIT = 40;
const SETTLED_HIDE_MS = 12_000;
const SKIP_ORDER: (keyof SkippedCounts)[] = ["hidden", "junk", "gitignore", "ooignore", "temporary", "links", "special", "unreadable"];

/** Active first, then failed, then the rest in the order they were queued. */
export function visibleItems(items: readonly UploadItem[], limit = ROW_LIMIT): UploadItem[] {
  const rank = (item: UploadItem) => (isUploadActive(item) ? (item.state === "copying" ? 0 : 1) : item.state === "failed" ? 2 : 3);
  const picked: UploadItem[] = [];
  for (const wanted of [0, 1, 2, 3]) {
    for (const item of items) {
      if (picked.length >= limit) return picked;
      if (rank(item) === wanted) picked.push(item);
    }
  }
  return picked;
}

/** Per-item progress of everything being added (25 §5: working states are visible), kept out of the way at the bottom right. */
export function UploadTray() {
  const { t, formatNumber } = useLocalization();
  const errorText = useErrorText();
  const state = useUploadQueueState();
  const [open, setOpen] = useState(true);
  const queue = getUploadQueue();
  // The counts come from the main process (exact, however many rows this list keeps); the list itself is the work in flight and the last few results.
  const summary = useMemo(() => {
    let queued = 0; let settled = 0; let failed = 0; let done = 0; let duplicates = 0; let found = 0; let scanning = false; let choosing = false;
    for (const batch of state.batches) {
      queued += batch.queued; failed += batch.counts.failed; done += batch.counts.done + batch.counts.skipped; duplicates += batch.counts.skipped; found += batch.found;
      settled += batch.counts.done + batch.counts.failed + batch.counts.skipped + batch.counts.cancelled;
      if (batch.phase === "scanning") scanning = true;
      if (batch.phase === "choice") choosing = true;
    }
    const active = Math.max(0, queued - settled);
    return { active, failed, done, duplicates, found, queued, settled, scanning, choosing, ratio: queued > 0 ? settled / queued : active > 0 || scanning ? null : 1 };
  }, [state.batches]);
  const rows = useMemo(() => visibleItems(state.items), [state.items]);
  const skippedCount = Object.values(state.skipped).reduce((sum, value) => sum + value, 0);
  const duplicates = summary.duplicates;
  const settled = summary.active === 0 && summary.failed === 0 && skippedCount === 0 && !summary.scanning && !summary.choosing;

  // A finished, clean list leaves by itself; one that skipped or failed something stays until it is closed.
  useEffect(() => {
    if (!settled || state.items.length === 0) return undefined;
    const timer = window.setTimeout(() => queue.dismiss(), SETTLED_HIDE_MS);
    return () => window.clearTimeout(timer);
  }, [settled, state.items.length, queue]);

  if (state.dismissed || (state.items.length === 0 && skippedCount === 0 && !state.error)) return null;
  const title = summary.scanning ? t("app.drop.tray.preparing", { count: summary.found })
    : summary.choosing ? t("app.drop.tray.choice", { count: summary.found })
      : summary.active > 0 ? (summary.queued > 1_000 ? t("app.drop.tray.progress", { done: summary.settled, total: summary.queued }) : t("app.sources.uploading", { count: summary.active }))
        : t("app.drop.tray.done", { count: summary.done });
  const hidden = Math.max(state.items.length - rows.length, summary.active - rows.filter((item) => isUploadActive(item)).length);

  return (
    <section className="so-tray" aria-label={t("app.drop.tray.label")} data-testid="upload-tray" data-active={summary.active} data-failed={summary.failed} data-done={summary.done} data-total={summary.queued} data-open={open ? "" : undefined}>
      <header className="so-tray-head">
        <span className="so-tray-title" data-testid="upload-tray-title">{title}</span>
        <span className="so-tray-tools">
          <IconButton icon={open ? ChevronDown : ChevronUp} label={open ? t("app.drop.tray.collapse") : t("app.drop.tray.expand")} size="compact" aria-expanded={open} onClick={() => setOpen((v) => !v)} data-testid="upload-tray-toggle" />
          <IconButton icon={X} label={t("app.common.close")} size="compact" onClick={() => queue.dismiss()} data-testid="upload-tray-close" />
        </span>
      </header>
      <ProgressBar label={title} value={summary.ratio} tone={summary.failed > 0 ? "warning" : "accent"} />
      {state.error && <p className="so-tray-error" role="alert">{errorText(state.error)}</p>}
      {open && (
        <>
          {rows.length > 0 && (
            <ul className="so-tray-list" data-testid="upload-items">
              {rows.map((item) => (
                <li key={item.id} className="so-tray-row" data-testid="upload-item" data-name={item.name} data-state={item.state}>
                  <FileIcon size={16} strokeWidth={1.5} className="so-icon so-weak" aria-hidden="true" />
                  <span className="so-tray-name">
                    <span className="so-truncate" title={item.relative}>{item.name}</span>
                    {item.folder && <span className="so-tray-sub so-truncate">{item.folder}</span>}
                  </span>
                  <span className="so-tray-state">
                    {item.state === "copying" || item.state === "waiting" ? (
                      <>
                        <span className="so-tray-sub">{item.state === "waiting" ? t("app.drop.item.waiting") : `${formatNumber(item.size > 0 ? Math.min(100, Math.round((item.bytesDone / item.size) * 100)) : 0)}%`}</span>
                        <IconButton icon={X} label={t("app.common.cancel")} size="compact" onClick={() => { void queue.cancel(item.id); }} />
                      </>
                    ) : item.state === "failed" ? (
                      <>
                        <StatusDot tone="danger" label={errorText({ code: item.code ?? "SO_UPLOAD_FAILED", retryable: true, details: {} })} />
                        <IconButton icon={RotateCw} label={t("app.common.retry")} size="compact" onClick={() => { void queue.retry(item.id); }} />
                      </>
                    ) : item.state === "skipped" ? <StatusDot tone="neutral" label={t("app.drop.item.duplicate")} />
                      : item.state === "cancelled" ? <StatusDot tone="neutral" label={t("app.drop.item.cancelled")} />
                        : <StatusDot tone="success" label={t("app.drop.item.done")} />}
                  </span>
                  {(item.state === "copying" || item.state === "waiting") && <span className="so-tray-meter"><ProgressBar label={item.name} value={item.size > 0 ? item.bytesDone / item.size : null} /></span>}
                </li>
              ))}
              {hidden > 0 && <li className="so-tray-more so-weak" data-testid="upload-more">{t("app.drop.tray.more", { count: hidden })}</li>}
            </ul>
          )}
          {(skippedCount > 0 || duplicates > 0) && (
            <div className="so-tray-skipped" data-testid="upload-skipped" data-count={skippedCount + duplicates}>
              <span className="so-tray-subtitle">{t("app.drop.skip.title", { count: skippedCount + duplicates })}</span>
              <ul className="so-tray-reasons">
                {SKIP_ORDER.filter((key) => state.skipped[key] > 0).map((key) => (
                  <li key={key} data-reason={key}><span>{t(`app.drop.skip.${key}`)}</span><span className="so-tray-count">{formatNumber(state.skipped[key])}</span></li>
                ))}
                {duplicates > 0 && <li data-reason="duplicate"><span>{t("app.drop.skip.duplicate")}</span><span className="so-tray-count">{formatNumber(duplicates)}</span></li>}
              </ul>
              {state.skippedSamples.length > 0 && <span className="so-tray-sub so-truncate">{state.skippedSamples.map((sample) => sample.name).join(" · ")}</span>}
              <span className="so-tray-sub">{t("app.drop.skip.why")}</span>
            </div>
          )}
        </>
      )}
    </section>
  );
}
