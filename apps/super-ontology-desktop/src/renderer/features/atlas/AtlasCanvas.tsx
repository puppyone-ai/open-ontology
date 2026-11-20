import { useEffect, useImperativeHandle, useRef, type Ref } from "react";
import { useLocalization } from "@open-ontology/design-system/localization";
import { AtlasController, type FrameStats } from "./controller";
import type { AtlasData, AtlasOp } from "./model";
import type { SceneNode } from "./scene";

export type AtlasCanvasHandle = {
  apply(ops: readonly AtlasOp[]): void;
  fit(): void;
  zoomIn(): void;
  zoomOut(): void;
  playGrowth(): number;
  search(query: string): SceneNode[];
  reveal(id: string): void;
  has(id: string): boolean;
  hasGroup(id: string): boolean;
  stats(): FrameStats | null;
  controller(): AtlasController | null;
};

type Props = {
  /** A new `data` replaces the whole picture (a mode change, a first load); live changes go through the handle. */
  data: AtlasData;
  reducedMotion: boolean;
  onOpen(node: SceneNode): void;
  onSelect(node: SceneNode | null): void;
  handle: Ref<AtlasCanvasHandle>;
};

/** The canvas of the atlas: sizing, pointer, wheel and keys, around an `AtlasController`. */
export function AtlasCanvas({ data, reducedMotion, onOpen, onSelect, handle }: Props) {
  const { t } = useLocalization();
  const wrapper = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const controller = useRef<AtlasController | null>(null);
  const callbacks = useRef({ onOpen, onSelect });
  callbacks.current = { onOpen, onSelect };

  useImperativeHandle(handle, () => ({
    apply: (ops) => controller.current?.apply(ops),
    fit: () => controller.current?.fit(),
    zoomIn: () => controller.current?.zoomBy(1.4),
    zoomOut: () => controller.current?.zoomBy(1 / 1.4),
    playGrowth: () => controller.current?.playGrowth() ?? 0,
    search: (query) => controller.current?.search(query) ?? [],
    reveal: (id) => { const node = controller.current?.scene.node(id); if (node) controller.current?.reveal(node); },
    has: (id) => controller.current?.scene.byId.has(id) ?? false,
    hasGroup: (id) => controller.current?.scene.byId.has(id) ?? false,
    stats: () => controller.current?.stats() ?? null,
    controller: () => controller.current,
  }), []);

  useEffect(() => {
    const element = canvas.current;
    const box = wrapper.current;
    if (!element || !box) return undefined;
    const instance = new AtlasController(element, data, {
      reducedMotion,
      onActivate: (node) => callbacks.current.onOpen(node),
      onSelect: (node) => callbacks.current.onSelect(node),
    });
    controller.current = instance;
    const measure = () => { const rect = box.getBoundingClientRect(); instance.resize(rect.width, rect.height, window.devicePixelRatio || 1); };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(box);
    const wheel = (event: WheelEvent) => instance.wheel(event);
    element.addEventListener("wheel", wheel, { passive: false });
    window.addEventListener("resize", measure);
    // A change of pixel ratio (the window moves to another screen, or the page is zoomed) does not resize the box: listen for it, so the canvas is never drawn blurry.
    let ratio: MediaQueryList | null = null;
    const watchRatio = () => {
      ratio?.removeEventListener("change", onRatio);
      ratio = window.matchMedia?.(`(resolution: ${window.devicePixelRatio || 1}dppx)`) ?? null;
      ratio?.addEventListener("change", onRatio);
    };
    function onRatio() { measure(); watchRatio(); }
    watchRatio();
    instance.start();
    return () => {
      observer.disconnect();
      element.removeEventListener("wheel", wheel);
      window.removeEventListener("resize", measure);
      ratio?.removeEventListener("change", onRatio);
      instance.dispose();
      controller.current = null;
    };
    // A new data set or a change of the motion preference builds a new controller; live updates never do.
  }, [data, reducedMotion]);

  return (
    <div className="so-atlas-canvas" ref={wrapper}>
      <canvas
        ref={canvas}
        className="so-atlas-surface"
        data-testid="atlas-canvas"
        data-cells={data.members.length}
        data-groups={data.groups.length}
        data-links={data.links.length}
        data-reduced={reducedMotion ? "true" : undefined}
        tabIndex={0}
        role="application"
        aria-label={t("app.atlas.canvas")}
        aria-describedby="so-atlas-hint"
        onPointerDown={(event) => controller.current?.pointerDown(event.nativeEvent)}
        onPointerMove={(event) => controller.current?.pointerMove(event.nativeEvent)}
        onPointerUp={(event) => controller.current?.pointerUp(event.nativeEvent)}
        onPointerCancel={(event) => controller.current?.pointerUp(event.nativeEvent)}
        onPointerLeave={() => controller.current?.pointerLeave()}
        onKeyDown={(event) => { if (controller.current?.key(event.nativeEvent)) event.preventDefault(); }}
      />
      <span id="so-atlas-hint" className="so-sr-only">{t("app.atlas.hint")}</span>
    </div>
  );
}
