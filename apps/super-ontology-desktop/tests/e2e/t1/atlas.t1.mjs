// Ontology atlas (D88): the picture is built from what the engine knows, the list view lists the same cells, and a member opens its entity.
import { runScenario } from "../harness/session.mjs";

const column = (table, name, position) => ({ urn: `oo:db/synthetic/${table}#${name}`, name, position, type: "int8", type_oid: 20 });
const table = (qualified, columns) => ({ urn: `oo:db/synthetic/${qualified}`, qualified_name: qualified, schema: qualified.split(".")[0], name: qualified.split(".")[1], column_count: columns.length, columns_status: "ok", columns: columns.map((name, index) => column(qualified, name, index + 1)) });
const end = (qualified, columns) => ({ table: qualified, urn: `oo:db/synthetic/${qualified}`, columns });
const synthetic = {
  protocol: "oo.ontology.schema/v1", source: { kind: "database", profile: "synthetic" },
  tables: [table("sales.customers", ["id", "name"]), table("sales.orders", ["id", "customer_id", "total"])],
  relationships: [{ urn: "oo:db/synthetic/relationship/orders_to_customers", id: "orders_to_customers", from: end("sales.orders", ["customer_id"]), to: end("sales.customers", ["id"]), cardinality: "many_to_one", provenance: "declared_fk" }],
  doc_links: [], total_tables: 2, next_cursor: null,
  sides: { db: { status: "ok", count: 2 }, relationships: { status: "ok", count: 1 }, docs: { status: "skipped", count: 0 } }, warnings: [], provenance: { kind: "gateway_schema", source: "synthetic" },
};

runScenario({ name: "atlas", title: "Ontology atlas: cells, list view and open an entity" }, async (s) => {
  await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
  // A fake engine is told its database side (docs-api 1.12); the real one has no Gateway profile here and the atlas shows that as a partial state.
  if (s.engine === "fake") await s.fakeDocs({ schema: synthetic });
  await s.openSeeded("ontology");
  await s.waitFor("screen-ontology");
  // The engine names the project "Project Alpha" or "alpha" depending on how its file arrived.
  const project = await s.waitForEntity(/^(project )?alpha$/u, { timeoutMs: 120_000 });
  await s.click("ontology-tab-atlas");
  await s.waitFor("atlas-canvas", {}, { timeoutMs: 60_000 });
  s.check("the atlas draws the entities the engine knows", Number(await s.attr("atlas-canvas", "data-cells")) > 0, String(await s.attr("atlas-canvas", "data-cells")));
  s.check("it groups them into cells by type", Number(await s.attr("atlas-canvas", "data-groups")) > 0);
  await s.click("atlas-view-list");
  await s.waitFor("atlas-list");
  s.check("the list view has a row per group", (await s.count("atlas-list-group")) > 0);
  if (s.engine === "fake") {
    // Source mode: the files of the source and the tables of the database, joined by the foreign key.
    await s.click("atlas-view-graph");
    await s.waitFor("atlas-canvas");
    await s.click("atlas-mode-source");
    await s.until(`document.querySelector('[data-testid="atlas-canvas"]')?.getAttribute('data-cells') >= 5`, "the files and both tables as cells", { timeoutMs: 60_000 });
    s.check("the atlas shows the tables of the database next to the files", true);
    s.check("a foreign key joins the two tables", Number(await s.attr("atlas-canvas", "data-links")) >= 1, String(await s.attr("atlas-canvas", "data-links")));
    await s.click("atlas-mode-type");
    await s.click("atlas-view-list");
    await s.waitFor("atlas-list");
  }
  // Open the groups one by one until the one that holds the project shows it (which type the engine gave it does not matter).
  await s.until(`(() => { const has = [...document.querySelectorAll('[data-testid="atlas-list-member"]')].some((member) => member.getAttribute("data-name") === ${JSON.stringify(project)}); if (!has) document.querySelector('[data-testid="atlas-list-group"][aria-expanded="false"]')?.click(); return has; })()`, "a group listing the project", { timeoutMs: 30_000 });
  s.check("a group lists its members by name", true);
  await s.click("atlas-list-member", { "data-name": project });
  await s.waitFor("screen-entity", { "data-name": project });
  s.check("a member opens its entity page", true);
  await s.shot("atlas-entity");
});
