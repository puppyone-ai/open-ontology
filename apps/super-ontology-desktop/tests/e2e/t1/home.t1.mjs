// Home: scale of the ontology (sources, entities, facts), freshness, what waits for a decision, the next step, recent changes.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "home", title: "Home shows the size and state of the ontology" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("home");
  await s.waitFor("screen-home");
  s.check("Home is the active section", (await s.attr("nav-home", "data-active")) === "true");
  await s.waitFor("statusbar-engine", { "data-state": "running" });
  await s.until("Number(document.querySelector('[data-testid=\"home-count-sources\"]')?.getAttribute('data-count')) === 1", "one source counted");
  s.check("one source is counted", true);
  await s.until("Number(document.querySelector('[data-testid=\"home-count-entities\"]')?.getAttribute('data-count')) >= 2", "entities counted", { timeoutMs: 90_000, interval: 1000 });
  await s.until("Number(document.querySelector('[data-testid=\"home-count-facts\"]')?.getAttribute('data-count')) >= 1", "facts counted", { timeoutMs: 90_000, interval: 1000 });
  s.check("entities and facts from the fixture files are counted", true);
  s.check("a next step is offered", await s.exists("home-next-step"));
  s.check("the jobs counter is a number", Number.isInteger(Number(await s.attr("statusbar-jobs", "data-count"))));
  await s.shot("home");
});
