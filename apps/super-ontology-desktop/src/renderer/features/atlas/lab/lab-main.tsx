import { ENTITY_TYPE_VALUES } from "../../../../shared/super-ontology/docs-api.generated";
import { createLabBridge, type LabBridge } from "./bridge";
import { syntheticAtlas } from "./fixtures";

/**
 * The full app on a synthetic corpus, for screenshots and recordings (`atlas-lab.html?n=400&groups=9&seed=7`). Opens straight on
 * Ontology > Atlas; `window.__lab` pushes engine events and swaps the corpus. Never part of a production build (not an entry of it).
 */
declare global { interface Window { __lab: LabBridge & { data: ReturnType<typeof syntheticAtlas> } } }

const q = new URLSearchParams(window.location.search);
const num = (name: string, fallback: number) => (q.has(name) ? Number(q.get(name)) : fallback);
const groups = num("groups", 9);
const data = syntheticAtlas({ groups, members: num("n", 400), linkRatio: num("links", 0.8), versions: num("versions", 8), seed: num("seed", 7), dbShare: num("db", 0.22), names: ENTITY_TYPE_VALUES.slice(0, Math.max(1, groups - Math.round(groups * num("db", 0.22)))) });
const lab = createLabBridge(data);
window.superOntology = lab.bridge;
window.__lab = Object.assign(lab, { data });
window.localStorage.setItem("so.firstRun.done", "1");
window.localStorage.setItem("so.atlas.mode", q.get("mode") === "source" ? "source" : "type");

await import("../../../main");
