import { useCallback, useEffect, useState } from "react";
import { invoke } from "../../model/bridgeInvoke";
import { mediaKindOf, utterancesOf, type MediaKind, type Utterance } from "../../model/details/mediaModel";
import type { EngineBlock, EngineFileDetail } from "../../../shared/super-ontology/bridge";

/** What the engine decided about a recording before telling its speakers apart (`parse.media` of the file detail). */
export type MediaClassInfo = { class: "conversation" | "non_conversation" | "unsure"; kind: string; source: string; speakersToName: number };

export type MediaSource =
  | { state: "loading" }
  | { state: "unavailable"; reason: "s3" | "other" }
  | { state: "ready"; fileId: string; name: string; path: string; kind: MediaKind; url: string; utterances: Utterance[]; info: MediaClassInfo | null; reload: () => void };

const PAGE = 500;
const PAGES_MAX = 40;

function infoOf(detail: EngineFileDetail): MediaClassInfo | null {
  const media = detail.parse?.media;
  if (!media || (media.class !== "conversation" && media.class !== "non_conversation" && media.class !== "unsure")) return null;
  return { class: media.class, kind: media.kind ?? "", source: media.source ?? "", speakersToName: media.speakers_to_name ?? 0 };
}

/**
 * One recording ready to play: where it plays from (a short-lived capability URL of the app's own protocol, never a path), what
 * kind of recording the engine took it for, and its transcript with times. Blocks are read page by page until the engine has no more.
 */
export function useMediaSource(fileId: string | null): MediaSource {
  const [source, setSource] = useState<MediaSource>({ state: "loading" });
  const [tick, setTick] = useState(0);
  const reload = useCallback(() => setTick((value) => value + 1), []);

  useEffect(() => {
    if (!fileId) { setSource({ state: "loading" }); return undefined; }
    let live = true;
    setSource((current) => (current.state === "ready" && current.fileId === fileId ? current : { state: "loading" }));
    void (async () => {
      const detail = await invoke("file-detail", { fileId });
      if (!live) return;
      if (!detail.ok) { setSource({ state: "unavailable", reason: "other" }); return; }
      const kind = mediaKindOf(typeof detail.value.format === "string" ? detail.value.format : null, detail.value.path);
      if (!kind) { setSource({ state: "unavailable", reason: "other" }); return; }
      const preview = await invoke("file-preview-url", { fileId });
      if (!live) return;
      if (!preview.ok) { setSource({ state: "unavailable", reason: preview.error.code === "SO_PREVIEW_UNAVAILABLE" ? "s3" : "other" }); return; }
      const blocks: EngineBlock[] = [];
      let cursor: string | undefined;
      for (let page = 0; page < PAGES_MAX; page += 1) {
        const result = await invoke("file-blocks", { fileId, limit: PAGE, ...(cursor ? { cursor } : {}) });
        if (!live) return;
        if (!result.ok) break;
        blocks.push(...result.value.items);
        cursor = result.value.next_cursor ?? undefined;
        if (!cursor) break;
      }
      setSource({ state: "ready", fileId, name: detail.value.path.slice(detail.value.path.lastIndexOf("/") + 1), path: detail.value.path, kind, url: preview.value.url, utterances: utterancesOf(blocks), info: infoOf(detail.value), reload });
    })();
    return () => { live = false; };
  }, [fileId, tick, reload]);

  return source;
}
