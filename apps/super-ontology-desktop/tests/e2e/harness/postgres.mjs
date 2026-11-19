// A throwaway PostgreSQL with TLS for the database-source scenario: one container, labelled `oo.lane=super-ontology-appe`, bound to
// 127.0.0.1 only, with a private CA and a server certificate for `localhost`. Synthetic data only. `stop()` removes the container.
import { execFile } from "node:child_process";
import fs from "node:fs/promises";
import path from "node:path";
import { promisify } from "node:util";

const run = promisify(execFile);
const LABEL = "oo.lane=super-ontology-appe";
const IMAGE = process.env.SO_T1_POSTGRES_IMAGE ?? "postgres:16.15-bookworm";
const sleep = (ms) => new Promise((resolve) => { setTimeout(resolve, ms); });

export async function dockerAvailable() {
  try { await run("docker", ["info", "--format", "{{.ServerVersion}}"], { timeout: 15_000 }); return true; } catch { return false; }
}

export async function startTlsPostgres(directory, { port } = {}) {
  const certs = path.join(directory, "certs");
  await fs.mkdir(certs, { recursive: true });
  const openssl = (...args) => run("openssl", args, { cwd: certs });
  await openssl("req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", "ca.key", "-out", "ca.crt", "-days", "2", "-subj", "/CN=so-t1-ca");
  await openssl("req", "-newkey", "rsa:2048", "-nodes", "-keyout", "server.key", "-out", "server.csr", "-subj", "/CN=localhost");
  await fs.writeFile(path.join(certs, "san.cnf"), "subjectAltName=DNS:localhost,IP:127.0.0.1\n");
  await openssl("x509", "-req", "-in", "server.csr", "-CA", "ca.crt", "-CAkey", "ca.key", "-CAcreateserial", "-out", "server.crt", "-days", "2", "-extfile", "san.cnf");
  const password = `pw-${Math.random().toString(36).slice(2)}-${Date.now().toString(36)}`;
  const name = `so-t1-pg-${process.pid}-${Date.now().toString(36)}`;
  const hostPort = port ?? 15000 + Math.floor(Math.random() * 20000);
  const script = "mkdir -p /ssl && cp /certs/server.crt /certs/server.key /ssl/ && chown postgres:postgres /ssl/* && chmod 600 /ssl/server.key && exec docker-entrypoint.sh postgres -c ssl=on -c ssl_cert_file=/ssl/server.crt -c ssl_key_file=/ssl/server.key";
  await run("docker", ["run", "-d", "--name", name, "--label", LABEL, "-p", `127.0.0.1:${hostPort}:5432`, "-e", `POSTGRES_PASSWORD=${password}`, "-e", "POSTGRES_DB=demo", "-v", `${certs}:/certs:ro`, "--entrypoint", "sh", IMAGE, "-c", script], { timeout: 120_000 });
  const stop = async () => { await run("docker", ["rm", "-f", "-v", name], { timeout: 60_000 }).catch(() => {}); };
  try {
    for (let attempt = 0; attempt < 90; attempt += 1) {
      const ready = await run("docker", ["exec", name, "psql", "-U", "postgres", "-d", "demo", "-tAc", "select 1"], { timeout: 10_000 }).then((r) => r.stdout.trim() === "1", () => false);
      if (ready) break;
      if (attempt === 89) throw new Error("postgres did not start");
      await sleep(1000);
    }
    await run("docker", ["exec", name, "psql", "-U", "postgres", "-d", "demo", "-c", "create table public.orders (id int primary key, customer text); insert into public.orders values (1, 'Acme'), (2, 'Globex');"], { timeout: 20_000 });
  } catch (error) { await stop(); throw error; }
  return { host: "127.0.0.1", port: hostPort, database: "demo", user: "postgres", password, caPem: await fs.readFile(path.join(certs, "ca.crt"), "utf8"), stop };
}
