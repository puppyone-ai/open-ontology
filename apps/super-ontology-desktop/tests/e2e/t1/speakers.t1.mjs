// Speakers: a conversation is already in the ontology under "Speaker A" and "Speaker B"; Review lists them with a play button each, the
// clip plays in the app's own audio element (its time moves), the right-hand pane follows along, and naming a speaker re-attributes what they said.
// Fake engine (no media sidecar needed); the recording is a real wav in a real Drive folder, served through the app's capability protocol.
import fs from "node:fs/promises";
import path from "node:path";
import { runScenario } from "../harness/session.mjs";

const DRIVE = "00000000-0000-4000-8000-0000000000d1";
const FILE = "00000000-0000-4000-8000-0000000000f1";
const PLACE_A = "《launch-sync》的说话人 A · abababab";
const PLACE_B = "《launch-sync》的说话人 B · abababab";

/** A mono 16 bit wav of a slowly changing tone, long enough for a few clips. */
function tone(seconds) {
  const rate = 8000;
  const samples = rate * seconds;
  const buffer = Buffer.alloc(44 + samples * 2);
  buffer.write("RIFF", 0); buffer.writeUInt32LE(36 + samples * 2, 4); buffer.write("WAVEfmt ", 8); buffer.writeUInt32LE(16, 16); buffer.writeUInt16LE(1, 20); buffer.writeUInt16LE(1, 22);
  buffer.writeUInt32LE(rate, 24); buffer.writeUInt32LE(rate * 2, 28); buffer.writeUInt16LE(2, 32); buffer.writeUInt16LE(16, 34); buffer.write("data", 36); buffer.writeUInt32LE(samples * 2, 40);
  for (let i = 0; i < samples; i += 1) buffer.writeInt16LE(Math.round(Math.sin((2 * Math.PI * (300 + 60 * Math.floor(i / rate / 4))) * i / rate) * 3000), 44 + i * 2);
  return buffer;
}

const ref = (id, key, type, name) => ({ id, key, type, name });
const utterance = (n, speaker, from, to, text) => ({ id: `blk_${n}`, ordinal: n, kind: "utterance", text, hpath: [], anchor: { kind: "media", t_start_ms: from * 1000, t_end_ms: to * 1000, speaker }, flags: [], src: `[src:meetings/launch-sync.wav#t=00:00:${String(from).padStart(2, "0")}-00:00:${String(to).padStart(2, "0")}@${speaker}]` });

runScenario({ name: "speakers", title: "Speakers: listen, then name", engine: "fake", timeoutMs: 240_000 }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  const folder = path.join(s.root, "meetings-drive");
  await fs.mkdir(path.join(folder, "meetings"), { recursive: true });
  await fs.writeFile(path.join(folder, "meetings", "launch-sync.wav"), tone(40));
  const now = new Date().toISOString();
  const spark = ref("00000000-0000-4000-8000-000000000a01", "project-spark", "Project", "Project Spark");
  const A = ref("00000000-0000-4000-8000-000000000a02", "speaker-a", "Person", PLACE_A);
  const B = ref("00000000-0000-4000-8000-000000000a03", "speaker-b", "Person", PLACE_B);
  const dana = ref("00000000-0000-4000-8000-000000000a04", "daniel-ortiz", "Person", "Daniel Ortiz");
  const evidence = { id: "00000000-0000-4000-8000-000000000b01", file_id: FILE, path: "meetings/launch-sync.wav", block_id: "blk_3", anchor: { kind: "media", t_start_ms: 17000, t_end_ms: 24000, speaker: "S1" }, src: "[src:meetings/launch-sync.wav#t=00:00:17-00:00:24@S1]", quote: "I will own Project Spark.", origin: "rule" };
  const fact = { id: "00000000-0000-4000-8000-000000000c01", fact_key: "c".repeat(64), subject: A, predicate: "owns", object: spark, confidence_bp: 9000, origin: "rule", status: "published", recorded_at: now, evidence: [evidence] };
  const item = (n, speaker, letter, candidates) => ({ id: `00000000-0000-4000-8000-000000000d0${n}`, kind: "speaker_identity", status: "pending", reason_code: "speaker_unconfirmed", drive_id: DRIVE, revision: 1, created_at: now,
    source: { file_id: FILE, path: "meetings/launch-sync.wav" }, subject: { b3: "ab".repeat(32), speaker, letter, placeholder: `《launch-sync》的说话人 ${letter} · abababab`, title: "launch-sync", candidates } });
  await s.fakeDocs({
    as_of_seq: 1,
    drives: [{ id: DRIVE, name: "Meetings", backend: "fs", config: { root: folder }, status: "active", revision: 1, counts: { published: 1 }, files_total: 1, bytes_total: 640000, pending_review: 2, created_at: now, model_enabled: false, freshness: { state: "waiting_review" } }],
    files: [{ id: FILE, drive_id: DRIVE, path: "meetings/launch-sync.wav", name: "launch-sync.wav", size: 640000, state: "published", format: "audio", state_since: now, retryable: false, pending_review: 2, modified_at: now,
      parse: { parser_id: "media", parser_version: "1", block_count: 4, warnings: [], media: { class: "conversation", kind: "conversation", source: "rules", speakers_to_name: 2 } } }],
    dirs: [{ drive_id: DRIVE, path: "meetings", files: 1, counts: { published: 1 } }],
    blocks: { [FILE]: [
      utterance(1, "S1", 0, 7, "Good morning everyone. This is the launch sync, and I am Daniel Ortiz."), utterance(2, "S2", 8, 16, "Thanks Daniel. Lens yield from the current supplier is seventy one percent."),
      utterance(3, "S1", 17, 24, "Understood. I will own Project Spark and report back on the threshold."), utterance(4, "S2", 25, 32, "Eighty five percent. We will ask for a new quote this week."),
    ] },
    entities: [{ ...A, counts: { facts: 1 }, facts: [fact], aliases: [] }, { ...B, counts: { facts: 0 }, facts: [], aliases: [] }, { ...dana, counts: { facts: 0 }, facts: [], aliases: [] }, { ...spark, counts: { facts: 1 }, facts: [], aliases: [] }],
    review: { items: [item(1, "S1", "A", [{ name: "Daniel Ortiz", source: "self_intro", quote: "I am Daniel Ortiz", block_id: "blk_1" }]), item(2, "S2", "B", [])] },
  });
  await s.evaluate("location.reload()");
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });

  // The ontology already knows the speakers by their placeholder (nothing waits for a name), shown in the person's language.
  await s.click("nav-ontology");
  await s.waitFor("entity-list");
  const placeholders = await s.attrs("entity-row", "data-name");
  s.check("the unnamed speakers are in the ontology under their placeholders", placeholders.includes(PLACE_A) && placeholders.includes(PLACE_B), placeholders.join(" | "));
  s.check("an unnamed speaker is shown as Speaker A of the meeting, not as the stored name", (await s.evaluate("[...document.querySelectorAll('[data-testid=\"entity-row\"]')].some((row) => /^Speaker A of “launch-sync”$/u.test(row.querySelector('.so-list-title')?.textContent ?? ''))")) === true);

  // Review: Speaker A and Speaker B with a play button each, the clip texts and the pane.
  await s.click("ontology-tab-review");
  await s.waitFor("speaker-group", {}, { timeoutMs: 30_000 });
  await s.waitFor("speaker-clip");
  s.check("Review lists Speaker A and Speaker B", (await s.attrs("speaker-row", "data-speaker")).join(",") === "S1,S2");
  s.check("a clip shows what is said in it", /Good morning everyone/u.test((await s.text("speaker-clip-text")) ?? ""));
  s.check("the preview pane shows the whole transcript", (await s.count("media-utterance")) === 4);
  await s.until("document.querySelector('[data-testid=\"media-class\"]')?.getAttribute('data-class') === 'conversation'", "the recording badge");
  s.check("the recording is badged as a conversation in the pane", true);

  // Playing: the audio element's currentTime moves. A first press may have to wait for the player to be ready.
  const readTime = () => s.evaluate("(() => { const e = document.querySelector('[data-testid=\"media-element\"]'); return e ? { t: e.currentTime, paused: e.paused, ready: e.readyState, error: e.error ? e.error.code : null, can: e.canPlayType('audio/wav') } : null; })()");
  s.note(`audio element before: ${JSON.stringify(await readTime())}`);
  await s.click("speaker-play");
  await s.until("(() => { const e = document.querySelector('[data-testid=\"media-element\"]'); return !!e && !e.paused && e.currentTime > 0.5; })()", "the media element plays and its time moves", { timeoutMs: 20_000 });
  const first = await readTime();
  await s.wait(700);
  const second = await readTime();
  s.check("playback starts: the element's time advances while it plays", second.t > first.t && !second.paused, `${first.t.toFixed(2)} -> ${second.t.toFixed(2)}`);
  s.check("the pane follows: a line is marked while it plays", (await s.count("media-utterance")) === 4 && (await s.evaluate("!!document.querySelector('[data-testid=\"media-utterance\"][aria-current=\"true\"]')")) === true);
  await s.shot("playing");
  // "Open at this point" jumps there.
  await s.click("speaker-open-at");
  await s.until("(() => { const e = document.querySelector('[data-testid=\"media-element\"]'); return !!e && e.currentTime >= 0 && e.currentTime < 3; })()", "open at the first clip");
  await s.click("speaker-play");
  await s.until("document.querySelector('[data-testid=\"media-pane\"]')?.getAttribute('data-playing') === 'false'", "pause");

  // Naming: type a known person's name for Speaker A; nothing else is blocked, and what A said is now Daniel Ortiz's.
  await s.type("speaker-name", "Daniel Ortiz");
  await s.click("speaker-confirm-typed");
  await s.until("document.querySelectorAll('[data-testid=\"speaker-row\"]').length === 1", "Speaker A's row leaves the list", { timeoutMs: 30_000 });
  const decisions = (await fs.readFile(path.join(s.stateDirectory, "fake-decisions.jsonl"), "utf8")).trim().split("\n").map((line) => JSON.parse(line));
  s.check("the app sent the name as an accepted speaker item", decisions.some((d) => d.items?.[0]?.decision === "accept" && d.items[0].edit?.name === "Daniel Ortiz"), JSON.stringify(decisions).slice(0, 200));
  await s.click("ontology-tab-entities");
  await s.waitFor("entity-list");
  await s.until("![...document.querySelectorAll('[data-testid=\"entity-row\"]')].some((row) => row.getAttribute('data-name') === '《launch-sync》的说话人 A · abababab')", "the placeholder merged away", { timeoutMs: 20_000 });
  await s.click("entity-row", { "data-name": "Daniel Ortiz" });
  await s.waitFor("screen-entity", { "data-name": "Daniel Ortiz" });
  await s.waitFor("fact-row");
  s.check("what Speaker A said is attributed to Daniel Ortiz now", /Daniel Ortiz/u.test((await s.text("fact-list")) ?? "") && (await s.attr("fact-row", "data-predicate")) === "owns");

  // The evidence of that fact is a time in the recording: one click plays it from there, in the same preview.
  await s.click("evidence-link");
  await s.waitFor("media-dialog", {}, { timeoutMs: 20_000 });
  await s.until("(() => { const e = document.querySelector('[data-testid=\"media-element\"]'); return !!e && e.currentTime >= 17 && e.currentTime < 30 && !e.paused; })()", "the evidence opens the recording at 0:17 and plays", { timeoutMs: 20_000 });
  s.check("an evidence mark plays the recording from its second", true);
  await s.shot("evidence-dialog");
});
