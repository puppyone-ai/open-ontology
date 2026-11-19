// Ontology atlas (D88): the picture is built from what the engine knows, the list view lists the same cells, and a member opens its entity.
import { runScenario } from "../harness/session.mjs";

runScenario({ name: "atlas", title: "Ontology atlas: cells, list view and open an entity" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  await s.openSeeded("ontology");
  await s.waitFor("screen-ontology");
  await s.waitFor("entity-row", { "data-name": "Project Alpha" }, { timeoutMs: 120_000 });
  await s.click("ontology-tab-atlas");
  await s.waitFor("atlas-canvas", {}, { timeoutMs: 60_000 });
  s.check("the atlas draws the entities the engine knows", Number(await s.attr("atlas-canvas", "data-cells")) > 0, String(await s.attr("atlas-canvas", "data-cells")));
  s.check("it groups them into cells by type", Number(await s.attr("atlas-canvas", "data-groups")) > 0);
  await s.click("atlas-view-list");
  await s.waitFor("atlas-list");
  s.check("the list view has a row per group", (await s.count("atlas-list-group")) > 0);
  await s.click("atlas-list-group");
  await s.waitFor("atlas-list-member", { "data-name": "Project Alpha" });
  s.check("a group lists its members by name", true);
  await s.click("atlas-list-member", { "data-name": "Project Alpha" });
  await s.waitFor("screen-entity", { "data-name": "Project Alpha" });
  s.check("a member opens its entity page", true);
  await s.shot("atlas-entity");
});
