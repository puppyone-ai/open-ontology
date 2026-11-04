#!/usr/bin/env python3
"""Isolated source-only checks, not T0-T2 Agent DB acceptance."""
import argparse
import base64
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import secrets
import shutil
import ssl
import subprocess
import time
import urllib.request

ROOT = Path(__file__).resolve().parents[3]
spec = importlib.util.spec_from_file_location("olap_fixture", ROOT / "testing/engines/olap/run.py")
olap = importlib.util.module_from_spec(spec)
spec.loader.exec_module(olap)


class Run(olap.Run):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)
        if self.family == "opensearch":
            for name in ("admin", "reader"):
                self.secrets[name] = "Aa9!" + secrets.token_hex(20)
                olap.private(self.private / (name + "-password"), self.secrets[name])

    def image(self):
        name = "mongo" if self.family == "mongodb" else self.family
        rows = json.loads((Path(__file__).parent / "images.lock.json").read_text())["images"]
        row = next(row for row in rows if row["id"].startswith(name + "-"))
        return row["ref"].rsplit(":", 1)[0] + "@" + row["platforms"]["linux/arm64"]["digest"]

    def start_mongodb(self):
        key = self.private / "replica-key"
        self.secrets["replica_key"] = base64.b64encode(secrets.token_bytes(512)).decode()
        olap.private(key, self.secrets["replica_key"])
        name = "oo-f-doc-" + self.id
        self.container(name, ["-p", "127.0.0.1::27017",
            "-v", f"{self.private}:/run/fixture:ro", self.image(),
            "bash", "-ec", "cp /run/fixture/replica-key /tmp/replica-key; "
            "chmod 600 /tmp/replica-key; chown mongodb:mongodb /tmp/replica-key; "
            "exec docker-entrypoint.sh mongod --replSet oo_rs --keyFile /tmp/replica-key --bind_ip_all"])
        for _ in range(120):
            code, _ = self.command(["docker", "exec", "-i", name, "mongosh", "--quiet"],
                                  "db.adminCommand({ping:1})", required=False, timeout=15)
            if code == 0:
                break
            _, running = self.command(["docker", "inspect", "--format", "{{.State.Running}}", name])
            if running.strip() != "true":
                raise RuntimeError("mongo exited during startup")
            time.sleep(1)
        else:
            raise RuntimeError("mongo startup")
        self.command(["docker", "exec", "-i", name, "mongosh", "--quiet"],
                     'rs.initiate({_id:"oo_rs",members:[{_id:0,host:"localhost:27017"}]})')
        script = """
const a = db.getSiblingDB("admin");
while (!a.hello().isWritablePrimary) sleep(200);
const fs = require("fs");
a.createUser({user:"admin",pwd:fs.readFileSync("/run/fixture/admin-password","utf8"),roles:["root"]});
a.auth("admin",fs.readFileSync("/run/fixture/admin-password","utf8"));
a.createRole({role:"source_reader",privileges:[
 {resource:{db:"fixture",collection:""},actions:["find","listCollections","listIndexes","changeStream"]},
 {resource:{db:"local",collection:"oplog.rs"},actions:["find"]},
 {resource:{cluster:true},actions:["replSetGetConfig"]}
],roles:[]});
a.createUser({user:"reader",pwd:fs.readFileSync("/run/fixture/reader-password","utf8"),roles:["source_reader"]});
print("reader_created");
"""
        self.command(["docker", "exec", "-i", name, "mongosh", "--quiet"], script, timeout=120)
        return self.port(name, 27017), None

    def certificates(self):
        # Test-only certs, confined to this run and removed during cleanup.
        for path in ("ca", "server"):
            args = ["openssl", "req", "-x509" if path == "ca" else "-new", "-newkey", "rsa:2048",
                    "-nodes", "-keyout", str(self.private / (path + ".key")),
                    "-out", str(self.private / (path + (".pem" if path == "ca" else ".csr"))),
                    "-subj", "/CN=oo-fixture-ca" if path == "ca" else "/CN=node-0.example.com"]
            if path == "ca":
                args += ["-days", "2", "-addext", "basicConstraints=critical,CA:TRUE",
                         "-addext", "keyUsage=critical,keyCertSign,cRLSign",
                         "-addext", "subjectKeyIdentifier=hash"]
            self.command(args)
        olap.private(self.private / "extensions",
                     "subjectAltName=IP:127.0.0.1,DNS:localhost,DNS:node-0.example.com\n"
                     "basicConstraints=critical,CA:FALSE\n"
                     "keyUsage=critical,digitalSignature,keyEncipherment\n"
                     "extendedKeyUsage=serverAuth,clientAuth\n"
                     "subjectKeyIdentifier=hash\nauthorityKeyIdentifier=keyid:always,issuer\n")
        self.command(["openssl", "x509", "-req", "-in", str(self.private / "server.csr"),
                      "-CA", str(self.private / "ca.pem"), "-CAkey", str(self.private / "ca.key"),
                      "-CAcreateserial", "-days", "2", "-extfile", str(self.private / "extensions"),
                      "-out", str(self.private / "server.pem")])
        # Container uid 1000 must read mounted files; parent remains owner-private.
        for path in ("ca.pem", "server.pem", "server.key"):
            (self.private / path).chmod(0o644)

    def request(self, port, method, path, body=None):
        headers = {"Authorization": "Basic " + base64.b64encode(
            ("admin:" + self.secrets["admin"] if self.family == "opensearch" else
             "elastic:" + self.secrets["admin"]).encode()).decode()}
        if body is not None:
            headers["Content-Type"] = "application/json"
            body = json.dumps(body).encode()
        request = urllib.request.Request(f"https://127.0.0.1:{port}{path}", body, headers, method=method)
        context = ssl.create_default_context(cafile=self.private / "ca.pem")
        with urllib.request.urlopen(request, context=context, timeout=30) as response:
            return json.load(response)

    def start_search(self):
        self.certificates()
        tls = self.private / "tls"
        tls.mkdir(mode=0o755)
        for filename in ("ca.pem", "server.pem", "server.key"):
            shutil.copyfile(self.private / filename, tls / filename)
            (tls / filename).chmod(0o644)
        name = "oo-f-doc-" + self.id
        if self.family == "elasticsearch":
            olap.private(self.private / "env", "ELASTIC_PASSWORD=" + self.secrets["admin"] + "\n")
            config = {
                "discovery.type": "single-node", "network.host": "0.0.0.0",
                "node.store.allow_mmap": False, "xpack.security.enabled": True,
                "xpack.security.http.ssl.enabled": True,
                "xpack.security.http.ssl.key": "fixture/server.key",
                "xpack.security.http.ssl.certificate": "fixture/server.pem",
                "xpack.security.http.ssl.certificate_authorities": ["fixture/ca.pem"],
            }
            # JSON is also a YAML mapping; values never include passwords.
            (self.private / "elasticsearch.yml").write_text(json.dumps(config))
            (self.private / "elasticsearch.yml").chmod(0o644)
            self.container(name, ["-p", "127.0.0.1::9200", "--env-file", str(self.private / "env"),
                "-e", "ES_JAVA_OPTS=-Xms512m -Xmx512m",
                "-v", f"{tls}:/usr/share/elasticsearch/config/fixture:ro",
                "-v", f"{self.private / 'elasticsearch.yml'}:/usr/share/elasticsearch/config/elasticsearch.yml:ro",
                self.image()])
        else:
            olap.private(self.private / "env", "OPENSEARCH_INITIAL_ADMIN_PASSWORD=" + self.secrets["admin"] + "\n")
            # Demo account bootstrap runs only on this disposable fixture. Replace
            # its certs before starting the service; product never skips TLS checks.
            command = (
                "plugins/opensearch-security/tools/install_demo_configuration.sh -y -i -s; "
                "cp /run/fixture/ca.pem config/root-ca.pem; "
                "cp /run/fixture/server.pem config/esnode.pem; "
                "cp /run/fixture/server.key config/esnode-key.pem; "
                "exec bin/opensearch -Ediscovery.type=single-node "
                "-Enode.store.allow_mmap=false -Enetwork.host=0.0.0.0"
            )
            self.container(name, ["-p", "127.0.0.1::9200", "--env-file", str(self.private / "env"),
                "-e", "discovery.type=single-node", "-e", "node.store.allow_mmap=false",
                "-e", "OPENSEARCH_JAVA_OPTS=-Xms512m -Xmx512m",
                "-v", f"{tls}:/run/fixture:ro", "--entrypoint", "/bin/bash",
                self.image(), "-ec", command])
        port = self.port(name, 9200)
        last_error = None
        for _ in range(180):
            try:
                self.request(port, "GET", "/")
                break
            except Exception as error:
                last_error = str(error)
                _, running = self.command(["docker", "inspect", "--format", "{{.State.Running}}", name])
                if running.strip() != "true":
                    raise RuntimeError("search exited during startup")
                time.sleep(1)
        else:
            raise RuntimeError("search startup " + str(last_error))
        self.request(port, "PUT", "/events", {"settings": {"number_of_shards": 2, "number_of_replicas": 0},
            "mappings": {"properties": {"key": {"type": "keyword"}, "value": {"type": "long"}}}})
        for i in range(200):
            self.request(port, "PUT", f"/events/_doc/{i}",
                         {"key": f"{i:04}", "value": i, "nested": {"empty": None}})
        self.request(port, "POST", "/events/_refresh")
        if self.family == "elasticsearch":
            self.request(port, "PUT", "/_security/role/source_reader",
                {"cluster": ["monitor"], "indices": [{"names": ["events"],
                    "privileges": ["read", "view_index_metadata"]}]})
            self.request(port, "PUT", "/_security/user/reader",
                {"password": self.secrets["reader"], "roles": ["source_reader"]})
        else:
            self.request(port, "PUT", "/_plugins/_security/api/roles/source_reader",
                {"cluster_permissions": ["cluster_monitor"],
                 "index_permissions": [{"index_patterns": ["events"], "allowed_actions":
                     ["read", "indices_monitor", "indices:admin/get", "indices:admin/mappings/get",
                      "indices:data/read/point_in_time/create", "indices:data/read/point_in_time/delete",
                      "indices:data/read/point_in_time/read"]}]})
            self.request(port, "PUT", "/_plugins/_security/api/internalusers/reader",
                {"password": self.secrets["reader"], "opendistro_security_roles": ["source_reader"]})
        return port, str(self.private / "ca.pem")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--engine", required=True, choices=["mongodb", "opensearch", "elasticsearch"])
    parser.add_argument("--run-id", required=True)
    args = parser.parse_args()
    if not args.run_id or any(c not in "abcdefghijklmnopqrstuvwxyz0123456789-_" for c in args.run_id):
        parser.error("run-id must be a safe lowercase slug")
    run = Run(args.engine, args.run_id, 64)
    outcome = {"engine": args.engine, "status": "failed", "acceptance": "source_only",
               "T0": "not_accepted", "T1": "not_accepted", "T2": "not_accepted", "T8": "not_accepted"}
    try:
        _, commit = run.command(["git", "-C", str(ROOT), "log", "-1", "--format=%H", "--",
                                 "Cargo.lock", "crates/oo-connectors"])
        outcome["connector_commit"] = commit.strip()
        outcome["connector_files"] = {
            str(path.relative_to(ROOT)): hashlib.sha256(path.read_bytes()).hexdigest()
            for path in [
                ROOT / "Cargo.lock", ROOT / "crates/oo-connectors/Cargo.toml",
                ROOT / "crates/oo-connectors/src/lib.rs", ROOT / "crates/oo-connectors/src/document.rs",
                ROOT / "crates/oo-connectors/src/mongodb/mod.rs",
                ROOT / "crates/oo-connectors/src/opensearch/mod.rs",
                ROOT / "crates/oo-connectors/tests/document_live.rs",
            ]
        }
        port, ca = run.start_mongodb() if args.engine == "mongodb" else run.start_search()
        config = {"engine": args.engine, "port": port, "ca_file": ca,
                  "reader_password": str(run.private / "reader-password"),
                  "admin_password": str(run.private / "admin-password"),
                  "cursor_path": str(run.private / "resume.json")}
        path = run.private / "fixture.json"
        olap.private(path, json.dumps(config))
        os.environ["OO_DOCUMENT_LIVE_CONFIG"] = str(path)
        test = "mongodb_snapshot_changes_and_retention" if args.engine == "mongodb" else "search_pit_ranges_and_digests"
        _, output = run.command(["cargo", "test", "--locked", "-p", "oo-connectors", "--features", "mongodb,opensearch",
                                 "--test", "document_live", test, "--", "--ignored", "--nocapture"], timeout=900)
        observations = [json.loads(line) for line in output.splitlines() if line.startswith("{")]
        versions = {row["version"] for row in observations if row.get("engine") == args.engine}
        rows = json.loads((Path(__file__).parent / "images.lock.json").read_text())["images"]
        name = "mongo" if args.engine == "mongodb" else args.engine
        expected = next(row["product_version"] for row in rows if row["id"].startswith(name + "-"))
        if versions != {expected}:
            raise RuntimeError("observed version does not match locked product version")
        outcome["version_actual"] = expected
        outcome["status"] = "passed"
    except Exception as error:
        outcome["failure"] = str(error) if isinstance(error, RuntimeError) else type(error).__name__
    finally:
        outcome["cleanup_failed"] = run.cleanup()
        if outcome["cleanup_failed"]:
            outcome["status"] = "failed"
        olap.write_json(run.path / "summary.json", outcome)
    print(json.dumps(outcome))
    return 0 if outcome["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
