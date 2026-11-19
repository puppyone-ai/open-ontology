// Ontology: search entities, open one, read its facts and jump from a fact to its evidence in the source.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "ontology", title: "Ontology: entities, facts and provenance" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("ontology");
  await s.waitFor("screen-ontology");
  await s.waitFor("entity-list");
  await s.waitFor("entity-row", { "data-name": "Project Alpha" }, { timeoutMs: 120_000 });
  s.check("entities recognised from the sources are listed with their type", (await s.attr("entity-row", "data-type")) !== null);

  await s.type("entity-search", "Dana");
  await s.waitFor("entity-row", { "data-name": "Dana Lee" }, { timeoutMs: 30_000 });
  s.check("search narrows the list", (await s.attrs("entity-row", "data-name")).every((name) => /dana/iu.test(name)), (await s.attrs("entity-row", "data-name")).join(","));

  await s.click("entity-row", { "data-name": "Dana Lee" });
  await s.waitFor("screen-entity", { "data-name": "Dana Lee" });
  await s.waitFor("fact-list");
  await s.waitFor("fact-row");
  s.check("the entity page lists facts", (await s.count("fact-row")) > 0);
  const file = await s.attr("evidence-link", "data-file");
  s.check("a fact names the file its evidence is in", Boolean(file), String(file));
  await s.click("evidence-link");
  await s.until("!!document.querySelector('[data-testid=\"screen-source-detail\"], [data-testid=\"file-panel\"]')", "the source of the fact");
  s.check("one step from a fact reaches its source", true);
  await s.shot("evidence");
});
