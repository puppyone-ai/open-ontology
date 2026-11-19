// Sources > Add source > Database against a local PostgreSQL with TLS (labelled container): the form saves it the way `oo source add postgres`
// does (password on stdin, kept as a 0600 file, only a file reference in the stack), the row shows, the connection test answers, remove cleans up.
import fs from "node:fs/promises";
import path from "node:path";
import { dockerAvailable, startTlsPostgres } from "../harness/postgres.mjs";
import { EXIT_SKIPPED, runScenario } from "../harness/session.mjs";

if (!(await dockerAvailable())) { console.error("no Docker: the database scenario needs a local PostgreSQL container"); process.exit(EXIT_SKIPPED); }

runScenario({ name: "database", title: "Database source: add, test, remove", timeoutMs: 280_000 }, async (s) => {
  const pg = await startTlsPostgres(s.root);
  try {
    await s.waitFor("app-root", {}, { timeoutMs: 60_000 });
    await s.engineUp();
    await s.click("nav-sources");
    await s.waitFor("screen-sources");
    await s.click("add-source");
    await s.click("add-source-database");
    await s.waitFor("database-dialog");

    // Empty form: the dialog says what is missing instead of closing.
    await s.click("add-source-confirm");
    s.check("an empty form is refused in place", (await s.exists("database-dialog")) && (await s.count("db-source-row")) === 0);

    await s.type("db-name", "orders-db");
    await s.type("db-host", pg.host);
    await s.type("db-port", String(pg.port));
    await s.type("db-database", pg.database);
    await s.type("db-user", pg.user);
    await s.type("db-password", pg.password);
    await s.type("db-ca", pg.caPem.trim());
    await s.type("db-tables", "public.orders");
    await s.click("add-source-confirm");
    await s.waitFor("db-source-row", { "data-name": "orders-db" }, { timeoutMs: 60_000 });
    s.check("the database is listed on Sources", true);
    s.check("the dialog closed", !(await s.exists("database-dialog")));

    // The credential never travels back: not in the page, a 0600 file on disk, only a reference in the stack.
    s.check("the password is not in the page", !(await s.evaluate(`document.documentElement.outerHTML.includes(${JSON.stringify(pg.password)})`)));
    const engineDir = path.join(s.userData, "engine");
    const stack = await fs.readFile(path.join(engineDir, "db", "stack.yaml"), "utf8");
    s.check("the stack carries no password", !stack.includes(pg.password) && stack.includes("file:"));
    const secrets = await fs.readdir(path.join(engineDir, "secrets"));
    const passwordFile = secrets.find((name) => name.startsWith("src_orders-db"));
    const caFile = secrets.find((name) => name === "db-orders-db-ca.pem");
    s.check("a password file and a trust anchor file exist", Boolean(passwordFile && caFile), secrets.join(","));
    for (const name of [passwordFile, caFile]) s.check(`${name} is 0600`, ((await fs.stat(path.join(engineDir, "secrets", name))).mode & 0o777) === 0o600);
    s.check("the password file holds the password", (await fs.readFile(path.join(engineDir, "secrets", passwordFile), "utf8")).trim() === pg.password);
    s.check("the placeholder source is gone", !stack.includes('"placeholder"'));
    await s.shot("database-listed");

    await s.click("db-source-test");
    await s.until("[...document.querySelectorAll('[data-testid=\"db-source-row\"] .so-badge')].some((b) => b.dataset.tone === 'success')", "the connection test says reachable", { timeoutMs: 30_000 });
    s.check("the connection test reaches the database", true);

    await s.click("db-source-remove");
    await s.click("db-source-remove-confirm");
    await s.until("document.querySelectorAll('[data-testid=\"db-source-row\"]').length === 0", "the database row is gone", { timeoutMs: 30_000 });
    const left = await fs.readdir(path.join(engineDir, "secrets"));
    s.check("removing deletes the saved password and certificate", !left.some((name) => name.startsWith("src_orders-db") || name === "db-orders-db-ca.pem"), left.join(","));
  } finally {
    await pg.stop();
  }
});
