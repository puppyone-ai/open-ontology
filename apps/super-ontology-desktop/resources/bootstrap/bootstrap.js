// First-run page (volume 25, section 3, step 1): the app finds `oo`, starts `oo serve` and shows the progress inline, with the
// engine state in the status bar. It is the whole UI of a package built without the workspace renderer, and it only talks to the
// engine through `window.superOntology`.
const bridge = window.superOntology;
const $ = (id) => document.getElementById(id);

const TEXT = {
  en: {
    lead: "Put your files in, see the ontology, hand it to an Agent.",
    engine: "Engine", jobs: (n) => `${n} running`,
    states: { idle: "Not started", probing: "Checking", starting: "Starting", healthy: "Running", degraded: "Running with a problem", restarting: "Restarting",
      stopped: "Stopped", stopping: "Stopping", crashed: "Not available", incompatible: "Not compatible", oo_missing: "Not available", docker_missing: "Not available",
      docker_stopped: "Not available", embedded_missing: "Not available", unsupported_platform: "Not available" },
    steps: { idle: "Getting ready", probing: "Looking for oo", starting: "Starting oo serve", restarting: "Restarting the engine", stopping: "Stopping the engine" },
    ready: "The engine is running", readyDetail: (s) => `oo ${s.preflight?.oo?.version ?? ""} from ${source(s)}${s.mode === "remote" ? `, remote ${s.remote?.host ?? ""}` : ""}`,
    failed: "The engine did not start", missing: "oo was not found", missingDetail: "Install oo or choose its location in Settings, then try again.",
    retry: "Try again", logs: "Show logs", start: "Start the engine",
    code: (c) => (c ? `Code: ${c}` : ""),
  },
  "zh-Hans": {
    lead: "放进资料,看到本体,交给 Agent。",
    engine: "引擎", jobs: (n) => `${n} 个进行中`,
    states: { idle: "未启动", probing: "检查中", starting: "启动中", healthy: "运行中", degraded: "运行中,有问题", restarting: "重启中",
      stopped: "已停止", stopping: "停止中", crashed: "不可用", incompatible: "版本不兼容", oo_missing: "不可用", docker_missing: "不可用",
      docker_stopped: "不可用", embedded_missing: "不可用", unsupported_platform: "不可用" },
    steps: { idle: "准备中", probing: "正在查找 oo", starting: "正在启动 oo serve", restarting: "正在重启引擎", stopping: "正在停止引擎" },
    ready: "引擎已运行", readyDetail: (s) => `oo ${s.preflight?.oo?.version ?? ""},来源:${source(s)}${s.mode === "remote" ? `,远端 ${s.remote?.host ?? ""}` : ""}`,
    failed: "引擎未能启动", missing: "没有找到 oo", missingDetail: "请安装 oo,或在设置中指定它的位置,然后重试。",
    retry: "重试", logs: "查看日志", start: "启动引擎",
    code: (c) => (c ? `错误码:${c}` : ""),
  },
};
const SOURCES = { en: { user: "your setting", path: "PATH", bundled: "this app" }, "zh-Hans": { user: "你的设置", path: "PATH", bundled: "本应用" } };
let language = /^zh\b/iu.test(navigator.language) ? "zh-Hans" : "en";
const source = (snapshot) => (SOURCES[language][snapshot.preflight?.oo?.source] ?? "-");
const tone = (status) => (status === "healthy" ? "ok" : ["starting", "probing", "restarting", "stopping"].includes(status) ? "busy" : status === "degraded" ? "warn" : ["idle", "stopped"].includes(status) ? "none" : "fail");
const FAILED = new Set(["crashed", "incompatible", "oo_missing", "docker_missing", "docker_stopped", "embedded_missing", "unsupported_platform", "stopped"]);

function actions(primary, secondary) {
  for (const [button, spec] of [[$("primary"), primary], [$("secondary"), secondary]]) {
    button.hidden = !spec;
    if (spec) { button.textContent = spec.label; button.onclick = spec.run; }
  }
}

function render(snapshot) {
  const t = TEXT[language];
  const status = snapshot.status;
  $("lead").textContent = t.lead;
  $("dot").dataset.tone = tone(status);
  $("engine-label").textContent = t.engine;
  $("engine-state").textContent = t.states[status] ?? status;
  const jobs = snapshot.health?.jobs?.active;
  $("jobs").hidden = typeof jobs !== "number";
  if (typeof jobs === "number") $("jobs").textContent = t.jobs(jobs);
  const progress = document.querySelector(".progress");
  progress.classList.remove("indeterminate", "done", "failed");
  if (status === "healthy" || status === "degraded") {
    progress.classList.add("done");
    $("step").textContent = t.ready;
    $("detail").textContent = t.readyDetail(snapshot);
    actions(null, null);
  } else if (status === "stopped" && !snapshot.code) {
    $("step").textContent = t.states.stopped;
    $("detail").textContent = "";
    progress.classList.add("failed");
    actions({ label: t.start, run: () => bridge.invoke("engine-start", {}) }, null);
  } else if (FAILED.has(status)) {
    progress.classList.add("failed");
    $("step").textContent = status === "oo_missing" ? t.missing : t.failed;
    $("detail").textContent = status === "oo_missing" ? t.missingDetail : t.code(snapshot.code);
    actions({ label: t.retry, run: () => bridge.invoke("engine-start", {}) }, { label: t.logs, run: () => bridge.invoke("engine-reveal-logs", { target: "logs" }) });
  } else {
    progress.classList.add("indeterminate");
    $("step").textContent = t.steps[status] ?? t.steps.idle;
    $("detail").textContent = "";
    actions(null, null);
  }
  document.documentElement.lang = language === "zh-Hans" ? "zh-Hans" : "en";
}

if (!bridge) {
  $("step").textContent = "The app bridge is unavailable.";
} else {
  let latest = null;
  const apply = (snapshot) => { if (!latest || snapshot.revision >= latest.revision) { latest = snapshot; render(snapshot); } };
  bridge.shell.readSettings().then((settings) => { if (settings.language === "en" || settings.language === "zh-Hans") language = settings.language; if (latest) render(latest); }).catch(() => {});
  bridge.subscribe(({ channel, payload }) => { if (channel === "engine-changed") apply(payload); });
  bridge.invoke("engine-read", {}).then((result) => { if (result.ok) apply(result.value); });
  $("engine").onclick = () => bridge.invoke("engine-reveal-logs", { target: "logs" });
}
