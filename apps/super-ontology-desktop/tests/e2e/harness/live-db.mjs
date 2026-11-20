// A labelled, synthetic database for the live T1 (databases-live): four tables, three foreign keys, a verified-TLS server, a
// read-only account and a password file that ends with a newline (the way a secret file written by hand does).
//
//   const db = await startLiveDatabase({ kind: "postgres" | "mysql", dir });  // db.live is the SO_T1_LIVE_DB document
//   await db.stop();
//
// Containers carry `oo.lane=super-ontology-appp` and are removed by `stop()`. Nothing here is a customer value: every row is generated.
import { execFile } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const LANE = "super-ontology-appp";
/** What the generated data holds; the live scenario checks the app's Query screen against these. */
export const LIVE_COUNTS = Object.freeze({ customers: 20, products: 10, orders: 60, order_items: 150 });

const sh = (file, args, options = {}) => run(file, args, { maxBuffer: 16 * 1024 * 1024, ...options });
const wait = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

async function freePort() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen(0, "127.0.0.1", () => { const { port } = server.address(); server.close(() => resolve(port)); });
  });
}

/** A CA and a server certificate for 127.0.0.1 / localhost with the serverAuth usage (macOS TLS requires it). */
async function makeCertificates(dir) {
  const ca = path.join(dir, "ca");
  await fs.mkdir(dir, { recursive: true, mode: 0o700 });
  const extension = path.join(dir, "server.ext");
  await fs.writeFile(extension, "subjectAltName=IP:127.0.0.1,DNS:localhost,DNS:host.docker.internal\nextendedKeyUsage=serverAuth\nkeyUsage=digitalSignature,keyEncipherment\nbasicConstraints=CA:FALSE\n");
  const caExt = path.join(dir, "ca.ext");
  await fs.writeFile(caExt, "basicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,cRLSign\n");
  await sh("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", `${ca}.key`, "-out", `${ca}.pem`, "-days", "3", "-subj", "/CN=Super Ontology live test CA", "-extensions", "v3_ca", "-addext", "basicConstraints=critical,CA:TRUE", "-addext", "keyUsage=critical,keyCertSign,cRLSign"]);
  await sh("openssl", ["req", "-newkey", "rsa:2048", "-nodes", "-keyout", path.join(dir, "server.key"), "-out", path.join(dir, "server.csr"), "-subj", "/CN=localhost"]);
  await sh("openssl", ["x509", "-req", "-in", path.join(dir, "server.csr"), "-CA", `${ca}.pem`, "-CAkey", `${ca}.key`, "-CAcreateserial", "-out", path.join(dir, "server.pem"), "-days", "3", "-extfile", extension]);
  await fs.chmod(path.join(dir, "server.key"), 0o644); // read by the container's user; the folder itself is private and the key lives three days
  return { caFile: `${ca}.pem`, certFile: path.join(dir, "server.pem"), keyFile: path.join(dir, "server.key") };
}

const POSTGRES_SQL = (password) => `
create table customers (id integer primary key, name text not null, region text not null);
create table products (id integer primary key, name text not null, price numeric(10,2) not null);
create table orders (id integer primary key, customer_id integer not null references customers(id), placed_at date not null, status text not null);
create table order_items (id integer primary key, order_id integer not null references orders(id), product_id integer not null references products(id), quantity integer not null);
insert into customers select g, 'Customer ' || g, (array['north','south','east','west'])[1 + g % 4] from generate_series(1, ${LIVE_COUNTS.customers}) g;
insert into products select g, 'Product ' || g, (g * 7.5)::numeric(10,2) from generate_series(1, ${LIVE_COUNTS.products}) g;
insert into orders select g, 1 + g % ${LIVE_COUNTS.customers}, date '2026-01-01' + g, (array['open','shipped','paid'])[1 + g % 3] from generate_series(1, ${LIVE_COUNTS.orders}) g;
insert into order_items select g, 1 + g % ${LIVE_COUNTS.orders}, 1 + g % ${LIVE_COUNTS.products}, 1 + g % 5 from generate_series(1, ${LIVE_COUNTS.order_items}) g;
create role reader login password '${password}';
grant connect on database shop to reader;
grant usage on schema public to reader;
grant select on all tables in schema public to reader;
grant pg_read_all_stats to reader;
`;

const MYSQL_SQL = (password) => `
create table customers (id int primary key, name varchar(64) not null, region varchar(16) not null) engine=InnoDB;
create table products (id int primary key, name varchar(64) not null, price decimal(10,2) not null) engine=InnoDB;
create table orders (id int primary key, customer_id int not null, placed_at date not null, status varchar(16) not null, foreign key (customer_id) references customers(id)) engine=InnoDB;
create table order_items (id int primary key, order_id int not null, product_id int not null, quantity int not null, foreign key (order_id) references orders(id), foreign key (product_id) references products(id)) engine=InnoDB;
insert into customers with recursive n(g) as (select 1 union all select g + 1 from n where g < ${LIVE_COUNTS.customers}) select g, concat('Customer ', g), elt(1 + g % 4, 'north', 'south', 'east', 'west') from n;
insert into products with recursive n(g) as (select 1 union all select g + 1 from n where g < ${LIVE_COUNTS.products}) select g, concat('Product ', g), g * 7.5 from n;
insert into orders with recursive n(g) as (select 1 union all select g + 1 from n where g < ${LIVE_COUNTS.orders}) select g, 1 + g % ${LIVE_COUNTS.customers}, date_add('2026-01-01', interval g day), elt(1 + g % 3, 'open', 'shipped', 'paid') from n;
insert into order_items with recursive n(g) as (select 1 union all select g + 1 from n where g < ${LIVE_COUNTS.order_items}) select g, 1 + g % ${LIVE_COUNTS.orders}, 1 + g % ${LIVE_COUNTS.products}, 1 + g % 5 from n;
create user 'reader'@'%' identified by '${password}' require ssl;
grant select on shop.* to 'reader'@'%';
grant process on *.* to 'reader'@'%';
`;

/** @param {{ kind: "postgres" | "mysql", dir: string, image?: string }} options */
export async function startLiveDatabase({ kind, dir, image }) {
  const certs = await makeCertificates(path.join(dir, "certs"));
  const port = await freePort();
  const name = `so-appp-${kind}-${crypto.randomBytes(3).toString("hex")}`;
  // Letters and digits only: the password goes through SQL and a shell-free argument list, never a file name.
  const password = crypto.randomBytes(12).toString("hex");
  const passwordFile = path.join(dir, "reader-password");
  await fs.writeFile(passwordFile, `${password}\n`, { mode: 0o600 });
  const label = ["--label", `oo.lane=${LANE}`];
  const stop = async () => { await run("docker", ["rm", "-f", "-v", name]).catch(() => undefined); };
  try {
    if (kind === "postgres") {
      const tag = image ?? "postgres:16.15-bookworm";
      await sh("docker", ["run", "-d", "--name", name, ...label, "-e", `POSTGRES_PASSWORD=${crypto.randomBytes(8).toString("hex")}`, "-e", "POSTGRES_DB=shop",
        "-p", `127.0.0.1:${port}:5432`, "-v", `${path.join(dir, "certs")}:/certs:ro`, "--entrypoint", "bash", tag, "-c",
        "mkdir -p /ssl && cp /certs/server.pem /certs/server.key /ssl/ && chown postgres:postgres /ssl/* && chmod 600 /ssl/server.key && exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/ssl/server.pem -c ssl_key_file=/ssl/server.key"]);
      for (let i = 0; i < 90; i += 1) { // the entrypoint restarts the server once after its init scripts
        const ready = await sh("docker", ["exec", name, "pg_isready", "-U", "postgres", "-d", "shop"]).then(() => true, () => false);
        if (ready) { await wait(1500); if (await sh("docker", ["exec", name, "pg_isready", "-U", "postgres", "-d", "shop"]).then(() => true, () => false)) break; }
        await wait(1000);
      }
      const sql = POSTGRES_SQL(password);
      const child = execFile("docker", ["exec", "-i", name, "psql", "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", "shop"]);
      child.stdin.end(sql);
      await new Promise((resolve, reject) => { child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`seeding exited ${code}`)))); });
    } else {
      const tag = image ?? "mysql:8.4.7";
      const rootPassword = crypto.randomBytes(8).toString("hex");
      await sh("docker", ["run", "-d", "--name", name, ...label, "-e", `MYSQL_ROOT_PASSWORD=${rootPassword}`, "-e", "MYSQL_DATABASE=shop", "-p", `127.0.0.1:${port}:3306`,
        "-v", `${path.join(dir, "certs")}:/certs:ro`, tag, "--ssl-ca=/certs/ca.pem", "--ssl-cert=/certs/server.pem", "--ssl-key=/certs/server.key", "--require-secure-transport=ON"]);
      for (let i = 0; i < 120; i += 1) {
        const ready = await sh("docker", ["exec", name, "mysql", "-uroot", `-p${rootPassword}`, "-N", "-e", "select 1", "shop"]).then(() => true, () => false);
        if (ready) break;
        await wait(1000);
      }
      const child = execFile("docker", ["exec", "-i", name, "mysql", "-uroot", `-p${rootPassword}`, "shop"]);
      child.stdin.end(MYSQL_SQL(password));
      await new Promise((resolve, reject) => { child.on("exit", (code) => (code === 0 ? resolve() : reject(new Error(`seeding exited ${code}`)))); });
    }
  } catch (error) { await stop(); throw error; }
  const live = {
    kind, host: "127.0.0.1", port, database: "shop", user: "reader", passwordFile, caFile: certs.caFile,
    include: ["public.customers", "public.products", "public.orders", "public.order_items"].map((entry) => (kind === "mysql" ? entry.replace("public.", "shop.") : entry)),
    counts: LIVE_COUNTS, container: name,
  };
  return { live, stop };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const [action, kind = "postgres", dir] = process.argv.slice(2);
  if (action !== "start" || !dir) { console.error("usage: live-db.mjs start postgres|mysql <private dir>"); process.exit(2); }
  const { live } = await startLiveDatabase({ kind, dir });
  process.stdout.write(`${JSON.stringify({ ...live, passwordFile: live.passwordFile })}\n`);
}
