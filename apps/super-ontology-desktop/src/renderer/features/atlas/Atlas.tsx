import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { Button, EmptyState, IconButton, Notice, Segmented, Skeleton } from "@open-ontology/design-system";
import { Maximize2, Network, Play, Search, ZoomIn, ZoomOut } from "lucide-react";
import { useApp } from "../../hooks/useApp";
import { superDriveApi } from "../../model/superDriveApi";
import { useSuperDriveSelector } from "../../model/SuperDriveProvider";
import { isEngineReady } from "../../model/superDriveStateModel";
import { atlasSource } from "./atlasApi";
import { diffAtlas, loadAtlas, type AtlasSource } from "./adapter";
import { AtlasCanvas, type AtlasCanvasHandle } from "./AtlasCanvas";
import { AtlasList, COUNT_KEY } from "./AtlasList";
import { opsForEvents } from "./events";
import { emptyAtlas, type AtlasData, type AtlasMember, type AtlasMode, type AtlasRef } from "./model";
import type { SceneNode } from "./scene";
import { useReducedMotion } from "./useReducedMotion";
import "./atlas.css";

/** What the inspector shows about the selected cell: a copy, so the scene can keep moving under it. */
type Inspected = { id: string; label: string; kind: "group" | "member"; groupKind: SceneNode["groupKind"]; groupLabel: string; tone: SceneNode["tone"]; count: number; weight: number; links: number; ref: AtlasRef | null };

const MODE_KEY = "so.atlas.mode";
const REFRESH_DELAY_MS = 1200;

type View = "graph" | "list";

/** The ontology version in the corner of the picture; when a new one is published its digits roll in (still, when motion is reduced). */
function VersionTick({ n, reduced }: { n: number; reduced: boolean }) {
  const previous = useRef(n);
  const ticking = previous.current !== n && !reduced;
  useEffect(() => { previous.current = n; }, [n]);
  return (
    <span className="so-atlas-version" data-testid="atlas-version" data-ticking={ticking ? "true" : undefined}>
      <span key={n} className="so-atlas-version-n">v{n}</span>
    </span>
  );
}

export function Atlas({ onOpenEntity, source = atlasSource, subscribe = superDriveApi.subscribePushes }: {
  onOpenEntity: (key: string) => void;
  /** Injectable for tests and the measuring page. */
  source?: AtlasSource;
  subscribe?: typeof superDriveApi.subscribePushes;
}) {
  const { t } = useLocalization();
  const app = useApp();
  const ready = useSuperDriveSelector((s) => isEngineReady(s.engine));
  const reduced = useReducedMotion();
  const [mode, setMode] = useState<AtlasMode>(() => (window.localStorage.getItem(MODE_KEY) === "source" ? "source" : "type"));
  const [view, setView] = useState<View>("graph");
  const [canvasData, setCanvasData] = useState<AtlasData | null>(null);
  const [live, setLive] = useState<AtlasData>(emptyAtlas);
  const [status, setStatus] = useState<"loading" | "ready" | "error">("loading");
  const [truncated, setTruncated] = useState(false);
  const [query, setQuery] = useState("");
  const [matches, setMatches] = useState<SceneNode[]>([]);
  const [selected, setSelected] = useState<Inspected | null>(null);
  const [tick, setTick] = useState(0);
  /** Built-in entity types are named in the person's language; sources keep their own names. */
  const named = useCallback((data: AtlasData): AtlasData => ({ ...data, groups: data.groups.map((g) => (g.kind === "type" ? { ...g, label: t(`super-drive.entity.type.${g.label}`) } : g)) }), [t]);
  const shownLive = useMemo(() => named(live), [named, live]);
  const shownCanvas = useMemo(() => (canvasData ? named(canvasData) : null), [named, canvasData]);
  const handle = useRef<AtlasCanvasHandle>(null);
  const liveRef = useRef<AtlasData>(live);
  const matchAt = useRef(0);

  // First load, and every change of mode: the whole picture is replaced.
  useEffect(() => {
    if (!ready) return undefined;
    const signal = { aborted: false };
    setStatus("loading");
    void loadAtlas(source, { mode, signal }).then(({ data, truncated: cut }) => {
      if (signal.aborted) return;
      liveRef.current = data;
      setLive(data);
      setCanvasData(data);
      setTruncated(cut);
      setSelected(null);
      setMatches([]);
      setStatus("ready");
    }).catch(() => { if (!signal.aborted) setStatus("error"); });
    return () => { signal.aborted = true; };
  }, [ready, mode, source, tick]);

  // Later changes arrive as events; a reload is diffed against what is drawn, so what appeared behind it buds and what left fades.
  useEffect(() => {
    if (!ready || status !== "ready") return undefined;
    let timer: ReturnType<typeof setTimeout> | null = null;
    let signal = { aborted: false };
    const refresh = () => {
      if (timer) return;
      timer = setTimeout(() => {
        timer = null;
        signal = { aborted: false };
        const mine = signal;
        void loadAtlas(source, { mode, signal: mine }).then(({ data }) => {
          if (mine.aborted) return;
          handle.current?.apply(diffAtlas(liveRef.current, data).map((op) => (op.type === "bud" && op.group?.kind === "type" ? { ...op, group: { ...op.group, label: t(`super-drive.entity.type.${op.group.label}`) } } : op)));
          liveRef.current = data;
          setLive(data);
        }).catch(() => undefined);
      }, REFRESH_DELAY_MS);
    };
    const stop = subscribe((push) => {
      if (push.channel !== "events") return;
      const ops = opsForEvents(push.payload.events, { mode, hasMember: (id) => handle.current?.has(id) ?? false, hasGroup: (id) => handle.current?.hasGroup(id) ?? false });
      const rest = ops.filter((op) => op.type !== "refresh");
      if (rest.length > 0) handle.current?.apply(rest);
      if (rest.length !== ops.length) refresh();
    });
    return () => { stop(); signal.aborted = true; if (timer) clearTimeout(timer); };
  }, [ready, status, mode, source, subscribe, t]);

  const changeMode = (next: AtlasMode) => { window.localStorage.setItem(MODE_KEY, next); setMode(next); setQuery(""); };

  const open = useCallback((ref: AtlasRef | null) => {
    if (!ref) return;
    if (ref.kind === "entity") onOpenEntity(ref.key);
    else if (ref.kind === "file") app.openFile(ref.fileId);
  }, [app, onOpenEntity]);

  const onSelect = useCallback((node: SceneNode | null) => {
    if (!node) { setSelected(null); return; }
    const scene = handle.current?.controller()?.scene;
    setSelected({
      id: node.id, label: node.label, kind: node.kind, groupKind: node.groupKind, groupLabel: scene?.owner(node).label ?? node.label, tone: node.tone,
      count: node.count, weight: node.weight, links: scene?.neighbors(node.idx).size ?? 0, ref: node.ref,
    });
  }, []);

  const search = (value: string) => {
    setQuery(value);
    matchAt.current = 0;
    setMatches(view === "graph" ? handle.current?.search(value) ?? [] : []);
  };
  const jump = (direction: 1 | -1) => {
    if (matches.length === 0) return;
    matchAt.current = (matchAt.current + direction + matches.length) % matches.length;
    handle.current?.reveal(matches[matchAt.current]!.id);
  };

  const hasData = live.groups.length > 0;
  const partial = live.partial;

  return (
    <div className="so-stack so-atlas" data-testid="atlas">
      <div className="so-toolbar">
        <form className="so-search" role="search" onSubmit={(event) => { event.preventDefault(); jump(1); }}>
          <Search size={16} strokeWidth={1.5} className="so-search-icon so-icon" aria-hidden="true" />
          <input className="so-input" type="search" data-testid="atlas-search" value={query} onChange={(event) => search(event.target.value)} placeholder={t("app.atlas.search")} aria-label={t("app.atlas.search")} />
        </form>
        {view === "graph" && query.trim() !== "" && <span className="so-weak so-atlas-matches" role="status" data-testid="atlas-matches">{t("app.atlas.matches", { count: matches.length })}</span>}
        <span className="so-atlas-spacer" />
        <Segmented<AtlasMode> label={t("app.atlas.group")} value={mode} onChange={changeMode} options={[{ value: "type", label: t("app.atlas.group.type"), testId: "atlas-mode-type" }, { value: "source", label: t("app.atlas.group.source"), testId: "atlas-mode-source" }]} />
        <Segmented<View> label={t("app.atlas.view")} value={view} onChange={(next) => { setView(next); setMatches([]); }} options={[{ value: "graph", label: t("app.atlas.view.graph"), testId: "atlas-view-graph" }, { value: "list", label: t("app.atlas.view.list"), testId: "atlas-view-list" }]} />
      </div>

      {partial.length > 0 && status === "ready" && (
        <Notice tone="warning" title={t("app.atlas.partial.title")} testId="atlas-partial">{partial.map((part) => t(`app.atlas.partial.${part}`)).join(" ")}</Notice>
      )}
      {truncated && status === "ready" && <Notice tone="accent" testId="atlas-truncated">{t("app.atlas.truncated")}</Notice>}

      {status === "error" ? (
        <Notice tone="danger" title={t("app.common.loadFailed")} action={<Button size="compact" onClick={() => setTick((n) => n + 1)}>{t("app.common.retry")}</Button>}>{t("app.atlas.error")}</Notice>
      ) : status === "loading" && !canvasData ? (
        <div className="so-atlas-stage" aria-busy="true"><Skeleton height="100%" radius={8} /></div>
      ) : !hasData ? (
        <div className="so-atlas-stage"><EmptyState icon={Network} title={t("app.atlas.empty.title")} description={t("app.atlas.empty.body")} /></div>
      ) : view === "list" ? (
        <div className="so-atlas-stage so-atlas-stage-list">
          <AtlasList data={shownLive} query={query} onOpen={(member: AtlasMember) => open(member.ref)} />
        </div>
      ) : (
        <div className="so-atlas-stage" data-busy={status === "loading" ? "true" : undefined}>
          {shownCanvas && <AtlasCanvas data={shownCanvas} reducedMotion={reduced} handle={handle} onOpen={(node) => open(node.ref)} onSelect={onSelect} />}
          <div className="so-atlas-overlay so-atlas-overlay-start">
            <Button size="compact" icon={Play} onClick={() => handle.current?.playGrowth()} disabled={reduced} data-testid="atlas-play">{t("app.atlas.play")}</Button>
            <span className="so-weak so-atlas-tally" data-testid="atlas-tally">{t("app.atlas.tally", { items: live.members.length, links: live.links.length })}</span>
            {live.versions.length > 0 && <VersionTick n={live.versions[live.versions.length - 1]!.n} reduced={reduced} />}
          </div>
          <div className="so-atlas-overlay so-atlas-overlay-end">
            <IconButton icon={ZoomIn} label={t("app.atlas.zoomIn")} shortcut="+" variant="secondary" size="compact" onClick={() => handle.current?.zoomIn()} />
            <IconButton icon={ZoomOut} label={t("app.atlas.zoomOut")} shortcut="-" variant="secondary" size="compact" onClick={() => handle.current?.zoomOut()} />
            <IconButton icon={Maximize2} label={t("app.atlas.fit")} shortcut="0" variant="secondary" size="compact" onClick={() => handle.current?.fit()} data-testid="atlas-fit" />
          </div>
          {selected && (
            <aside className="so-atlas-inspector" data-testid="atlas-inspector" data-tone={selected.tone}>
              <p className="so-atlas-inspector-title so-truncate">{selected.label}</p>
              <p className="so-weak so-atlas-inspector-sub so-truncate">{selected.kind === "group" ? t(COUNT_KEY[selected.groupKind], { count: selected.count }) : selected.groupLabel}</p>
              {selected.kind === "member" && selected.weight > 0 && <p className="so-atlas-inspector-line">{t("app.ontology.facts", { count: selected.weight })}</p>}
              {selected.links > 0 && <p className="so-atlas-inspector-line">{t("app.atlas.linked", { count: selected.links })}</p>}
              {selected.ref && selected.ref.kind !== "table" && <Button size="compact" variant="primary" onClick={() => open(selected.ref)} data-testid="atlas-open">{selected.ref.kind === "entity" ? t("app.atlas.open.entity") : t("app.atlas.open.file")}</Button>}
            </aside>
          )}
          <span className="so-sr-only" role="status">{reduced ? t("app.atlas.static") : ""}</span>
        </div>
      )}
    </div>
  );
}
