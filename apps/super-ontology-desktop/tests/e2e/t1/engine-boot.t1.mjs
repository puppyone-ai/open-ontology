// The base of every other scenario: the built app opens, the bridge is there, the engine the app starts reaches healthy and is stopped
// again when the app quits. It reads the engine through the app's own bridge, not through test ids, so it runs on any renderer.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "engine-boot", title: "App boots and starts its engine" }, async (s) => {
  await s.until("typeof window.superOntology?.invoke === 'function'", "the bridge", { timeoutMs: 30_000 });
  s.check("the bridge exposes invoke, subscribe and shell", await s.evaluate("['invoke','subscribe','shell','pathsForFiles','openExternal'].every((k) => k in window.superOntology)"));
  await s.until("window.superOntology.invoke('engine-read', {}).then((r) => r.ok && r.value.status === 'healthy')", "a healthy engine", { timeoutMs: 150_000, interval: 1000 });
  const snapshot = await s.evaluate("window.superOntology.invoke('engine-read', {}).then((r) => r.value)");
  s.check("the engine is owned by the app and healthy", snapshot.ownedByDesktop === true && snapshot.status === "healthy", `${snapshot.status} owned=${snapshot.ownedByDesktop}`);
  s.check("no token reaches the renderer", !JSON.stringify(snapshot).includes("oodt_"));
  const health = await s.engineApi("/v1/docs/health");
  s.check("the control API answers with the token the engine wrote", health.status === 200 && health.body?.status === "ok", `${health.status} ${health.body?.status}`);
  const drives = await s.evaluate("window.superOntology.invoke('drives-list', {}).then((r) => r.ok ? r.value.length : -1)");
  s.check("a fresh profile has no source yet", drives === 0, String(drives));
  await s.shot("booted");
});
