// Settings: the engine (state, stop, start), and About. The status bar follows the engine.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "settings", title: "Settings: engine controls and About" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.engineUp();
  await s.click("nav-settings");
  await s.waitFor("screen-settings");
  await s.waitFor("engine-status", { "data-state": "healthy" });
  s.check("the engine is healthy and owned by the app", true);

  await s.click("engine-stop");
  await s.waitFor("engine-status", { "data-state": "stopped" }, { timeoutMs: 60_000 });
  await s.waitFor("statusbar-engine", { "data-state": "unavailable" });
  s.check("stopping the engine shows in Settings and in the status bar", true);

  await s.click("engine-start");
  await s.waitFor("engine-status", { "data-state": "healthy" }, { timeoutMs: 150_000 });
  await s.waitFor("statusbar-engine", { "data-state": "running" });
  s.check("starting it again brings it back", true);

  await s.click("engine-restart");
  await s.waitFor("engine-status", { "data-state": "healthy" }, { timeoutMs: 150_000 });
  s.check("a restart ends healthy", true);

  await s.waitFor("about-version");
  s.check("About names a version", /\d+\.\d+\.\d+/u.test(await s.text("about-version")), (await s.text("about-version")).trim());
  await s.shot("settings");
});
