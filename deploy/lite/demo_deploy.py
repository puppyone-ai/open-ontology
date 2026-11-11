"""Demo adapter around the installed lite loader, compiler and publisher.

Only the new deployment's synthetic source is administered. Model egress is
never started. Runtime diagnostics stay in private state, not in CLI output.
"""
import json
import hashlib
import os
from pathlib import Path
import re
import secrets
import stat
import subprocess
import sys

import demo_runtime as assets
import lifecycle as lite

def read_audit(path):
    # Gateway WAL records are non-secret metadata created with the process
    # umask (often 0644), inside an owner-only directory. Keep credential
    # reads on the stricter 0600 path; never follow links or accept writable WAL.
    lite.private_dir(path.parent)
    fd = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    with os.fdopen(fd, "rb") as stream:
        info = os.fstat(stream.fileno())
        if (not stat.S_ISREG(info.st_mode) or info.st_uid != os.getuid()
                or info.st_nlink != 1 or info.st_mode & 0o022 or info.st_size > 16384):
            raise ValueError("DEMO_AUDIT_FILE")
        return stream.read(16385)


def command(argv, data=None, timeout=600, candidate=False):
    result = subprocess.run(argv, input=data, capture_output=True, timeout=timeout)
    if result.returncode:
        # The normal retained build also treats this single refusal as a
        # prepared candidate, never as an activated publication.
        if not (candidate and result.returncode == 1
                and json.loads(result.stderr).get("error") == "publication_semantic_admission_required"
                and json.loads(result.stdout).get("state") == "pending"):
            raise ValueError("DEMO_RUNTIME_COMMAND")
    return result.stdout


class Deployment:
    def __init__(self, root):
        self.root = root
        self.owner = json.loads(lite.read_private(root / "owner.json"))
        self.project = self.owner["project"]
        if self.owner["lane"] != "v03":
            raise ValueError("DEMO_OWNER")
        expected = "oo-a-lite-" + hashlib.sha256(str(root).encode()).hexdigest()[:16]
        if self.project != expected:
            raise ValueError("DEMO_OWNER_BINDING")
        self.image = self.owner["image"]
        self.env = dict(os.environ, OO_LITE_STATE=str(root), OO_LITE_RUNTIME_IMAGE=self.image,
                        OO_LITE_UID=str(os.getuid()), OO_LITE_GID=str(os.getgid()),
                        OO_LITE_LANE="v03", OO_LITE_SUBNET=self.owner["subnet"])
        self.compose = lite.docker_command(["compose", "--project-name", self.project,
                        "-f", str(root / "compose.yaml"), "-f", str(root / "compose.local.yaml")], self.env)

    def runtime(self, argv, data=None, candidate=False):
        platform = command(["docker", "image", "inspect", "--format",
                            "{{.Os}}/{{.Architecture}}", self.image]).decode().strip()
        if platform not in ("linux/arm64", "linux/amd64"):
            raise ValueError("DEMO_IMAGE_PLATFORM")
        # Docker's implicit-emulation warning would corrupt the closed JSON refusal.
        return command(["docker", "run", "--platform", platform, "--rm", "-i", "--label", "oo.lane=v03",
                        "--label", "oo.lite.project=" + self.project,
                        "--network", self.project + "_default",
                        "--user", f"{os.getuid()}:{os.getgid()}",
                        "--read-only", "--tmpfs", "/tmp", "--cap-drop", "ALL",
                        "--security-opt", "no-new-privileges:true",
                        "-v", f"{self.root}/work:/work",
                        "-v", f"{self.root}/home:/home/oo:ro",
                        "-v", f"{self.root}/pki:/pki:ro",
                        "-e", "HOME=/home/oo", "--entrypoint", argv[0], self.image, *argv[1:]],
                       data, candidate=candidate)

    def sql(self, service, sql, database="postgres"):
        result = subprocess.run(self.compose + ["exec", "-T", service, "psql", "-X", "-At",
                                "-v", "ON_ERROR_STOP=1", "-U", "postgres", "-d", database],
                                input=sql.encode(), env=self.env, capture_output=True, timeout=60)
        if result.returncode:
            raise ValueError("DEMO_ADMIN_SQL")
        return result.stdout.decode().strip()

    def resources(self):
        found = []
        for kind in ("container", "volume", "network"):
            ids = command(["docker", kind, "ls", "-q", *(["-a"] if kind == "container" else []),
                           "--filter", "label=com.docker.compose.project=" + self.project]).decode().split()
            for id_ in ids:
                info = json.loads(command(["docker", kind, "inspect", id_]))[0]
                labels = info.get("Config", {}).get("Labels", {}) if kind == "container" else info["Labels"]
                if labels.get("oo.lane") != "v03":
                    raise ValueError("DEMO_OWNERSHIP")
                found.append((kind, id_))
        return found

    def down(self):
        owned = self.resources()  # Validate every resource before removing any.
        for kind, id_ in owned:
            command(["docker", kind, "rm", *(["-f"] if kind == "container" else []), id_])
        if self.resources():
            raise ValueError("DEMO_CLEANUP_INCOMPLETE")
        return dict(status="stopped", resources=[])

    def collect_audit(self, queries):
        """Copy actual Gateway WAL records into the demo control DB, then read back.

        This is a local-demo collector, not a general production audit service.
        No event is synthesized from a query result or its expected gold value.
        """
        if not isinstance(queries, list) or len(queries) != 4:
            raise ValueError("DEMO_AUDIT_REQUEST")
        publication = json.loads(lite.read_private(self.root / "work/demo-publication.json"))
        receipt = publication["receipt_sha256"]
        if not re.fullmatch("[0-9a-f]{64}", receipt):
            raise ValueError("DEMO_PUBLICATION_BINDING")
        if self.sql("agent-db", "SELECT count(*) FROM _oo.native_library_head "
                    f"WHERE library_id='demo' AND state='ready' AND receipt_sha256='{receipt}';") != "1":
            raise ValueError("DEMO_PUBLICATION_BINDING")
        inserts, keys = [], []
        for query in queries:
            qid = query["query_id"]
            if not isinstance(qid, str) or not re.fullmatch(r"q_[A-Za-z0-9]{1,64}", qid):
                raise ValueError("DEMO_AUDIT_REQUEST")
            phases = ["refuse"] if query["denied"] else ["admit", "complete"]
            for phase in phases:
                path = self.root / "work/operator/gateway/collector" / f"{qid}-{phase}.wal"
                raw = read_audit(path)
                row = json.loads(raw)
                if row.get("query_id") != qid or row.get("phase") != phase or row.get("principal") != "analyst":
                    raise ValueError("DEMO_AUDIT_BINDING")
                if phase != "complete" and (row.get("project") != "demo"
                        or row.get("role") != "oo_r_analyst"
                        or row.get("policy_id") != publication["config_digest"]):
                    raise ValueError("DEMO_AUDIT_BINDING")
                if (phase == "refuse" and row.get("code") != "COLUMN_NOT_ALLOWED"
                        or phase == "complete" and row.get("status") != "succeeded"):
                    raise ValueError("DEMO_AUDIT_OUTCOME")
                key = f"{qid}/{phase}"
                inserts.append(f"INSERT INTO oo_demo.audit_event VALUES ('{key}', "
                               f"convert_from(decode('{raw.hex()}','hex'),'UTF8')::jsonb) "
                               "ON CONFLICT DO NOTHING;")
            keys.append(f"'{qid}/{phases[-1]}'")
        sql = ("BEGIN; CREATE SCHEMA IF NOT EXISTS oo_demo; REVOKE ALL ON SCHEMA oo_demo FROM PUBLIC;"
               "CREATE TABLE IF NOT EXISTS oo_demo.audit_event(audit_id text PRIMARY KEY, payload jsonb NOT NULL);"
               "REVOKE ALL ON oo_demo.audit_event FROM PUBLIC;" + "".join(inserts) + "COMMIT;"
               "SELECT json_agg(json_build_object('audit_id',audit_id,'query_id',payload->>'query_id',"
               "'status',payload->>'status','code',payload->>'code')) FROM oo_demo.audit_event "
               f"WHERE audit_id IN ({','.join(keys)});")
        rows = json.loads(self.sql("control-db", sql).splitlines()[-1])
        if len(rows) != 4:
            raise ValueError("DEMO_AUDIT_PERSISTENCE")
        return dict(status="verified_control_db", events=rows,
                    published_generation=publication["generation"], receipt_sha256=receipt)

    def deploy(self, asset_root):
        assets.verify_assets(asset_root)
        work = self.root / "work"
        source_store = self.root / "home/.config/open-ontology/secrets"
        password = secrets.token_hex(32)
        # Deliberately separate source database; the Agent DB holds only replicas.
        self.sql("control-db", "CREATE DATABASE logistics;")
        self.sql("control-db", (asset_root / "seed.sql").read_text(), "logistics")
        self.sql("control-db", "SET log_statement='none'; SET log_min_error_statement='panic';"
                 f"ALTER ROLE oo_demo_reader PASSWORD '{password}';"
                 # The connector sets its own read-only session guards and
                 # rejects startup overrides. SELECT-only ACLs still deny writes.
                 "ALTER ROLE oo_demo_reader RESET default_transaction_read_only;"
                 "GRANT pg_read_all_stats TO oo_demo_reader WITH INHERIT TRUE;"
                 # Capture the whole synthetic source. The serving replica
                 # receives the role filters before its publication is opened.
                 "ALTER TABLE demo.orders DISABLE ROW LEVEL SECURITY;", "logistics")
        lite.write(source_store / "demo-source", password.encode())
        lite.copy_private(self.root / "pki/lite-ca-cert", source_store / "demo-ca")
        stack = work / "build/stack.json"
        common = ["--stack", "/work/build/stack.json", "--source", "demo",
                  "--target", "/work/operator/target.json",
                  "--secrets-dir", "/home/oo/.config/open-ontology/secrets"]
        if json.loads(lite.read_private(stack))["sources"][0]["host"] != "control-db":
            raise ValueError("DEMO_SOURCE_BINDING")
        loaded = self.runtime(["oo-load", *common, "--run-id", "demo-v1"])
        lite.write(work / "demo-load.json", loaded)
        self.install_demo_policy()
        return self.publish(asset_root, common)

    def publish(self, asset_root, common):
        work = self.root / "work"
        candidate = self.runtime(["oo-load", "publish", "--generation", "demo-v1",
                                  "--library", "demo", *common], candidate=True)
        lite.write(work / "demo-candidate.json", candidate)
        self.sql("agent-db", "CREATE ROLE oo_r_analyst NOLOGIN NOINHERIT NOBYPASSRLS;"
                 "GRANT oo_r_analyst TO oo_gateway_login WITH INHERIT FALSE, SET TRUE;"
                 "GRANT EXECUTE ON FUNCTION _oo.native_library_retained_table(text) TO oo_gateway_login;")
        envelope = json.loads((asset_root / "confirmed-analyst.json").read_bytes())
        compiled = json.loads(self.runtime(["selfbuild-budget"],
                              json.dumps(dict(operation="reviewed", command=envelope)).encode()))
        if compiled["producer"]["artifact"] != json.loads((asset_root / "pack-analyst.json").read_bytes()):
            raise ValueError("DEMO_PACK_BINDING")
        reviewed = dict(protocol="oo.owner-reviewed-registry/v1", status="reviewed",
                        published=False, authentication="not_claimed", command=envelope,
                        compiled=compiled, review_decision=envelope["confirmation"])
        request = dict(protocol="oo.release-coordinator/publish-retained/v1",
                       library_id="demo", generation="demo-v1", login="oo_gateway_login",
                       reviewed_registry=reviewed)
        receipt = json.loads(self.runtime(
            ["release-coordinator", "publish-retained", "--config", "/work/operator/publisher.json"],
            json.dumps(request).encode()))
        if receipt.get("state") != "ready" or receipt.get("outcome") not in ("activated", "replay"):
            raise ValueError("DEMO_PUBLICATION_REFUSED")
        lite.save_json(work / "demo-publication.json", receipt)
        gateway = work / "operator/gateway"
        lite.private_dir(gateway / "reviewed/library")
        document = receipt["gateway_document"]
        lite.save_json(gateway / "packs/demo.json", document["envelope"]["producer"]["artifact"])
        lite.save_json(gateway / "reviewed/library/library.json", document)
        result = subprocess.run(self.compose + ["--profile", "integration", "up", "-d", "--no-deps", "gateway"],
                                env=self.env, capture_output=True, timeout=120)
        if result.returncode:
            raise ValueError("DEMO_GATEWAY_START")
        public = f'https://localhost:{self.owner["gateway_port"]}'
        grant = dict(kind="client", project_id="demo", principal_id="analyst", role="oo_r_analyst",
                     claims=[], policy_version="demo-v1", knowledge_version="demo-v1",
                     audience_url=public, purpose="public synthetic demo")
        lite.save_json(work / "demo-grant.json", grant)
        self.runtime(["issue-token", "issue", "/pki/operator-endpoint.json",
                      "/pki/token-admin-password", "/pki/lite-ca-cert",
                      "/work/demo-grant.json", "/work/demo-token"])
        return dict(status="published", gateway=public, local_fixture=True, model_calls=0)

    def install_demo_policy(self):
        """Apply the fixture's region policy to the replica, before publication.

        A non-bypass view owner makes PostgreSQL enforce RLS through the retained
        view. Neither the Gateway login nor its roles can access the base tables.
        """
        self.sql("agent-db", """
CREATE ROLE oo_demo_view_owner NOLOGIN NOINHERIT NOSUPERUSER NOBYPASSRLS;
GRANT USAGE ON SCHEMA _oo_load TO oo_demo_view_owner;
DO $policy$
DECLARE g record; part jsonb; stage text;
BEGIN
  FOR g IN SELECT relation_name, recipe FROM _oo_load.generations
           WHERE run='demo-v1' AND recipe->'plan'->'table'->>'name'='orders'
  LOOP
    FOR part IN SELECT value FROM jsonb_array_elements(g.recipe->'parts') LOOP
      stage := part->>'stage';
      EXECUTE format('ALTER TABLE _oo_load.%I ENABLE ROW LEVEL SECURITY', stage);
      EXECUTE format('ALTER TABLE _oo_load.%I FORCE ROW LEVEL SECURITY', stage);
      EXECUTE format('CREATE POLICY demo_region ON _oo_load.%I FOR SELECT TO oo_demo_view_owner
        USING ((current_setting(''role'')=''oo_r_analyst'' AND region_id=1)
            OR (current_setting(''role'')=''oo_r_ops'' AND region_id=2))', stage);
      -- Publication recursively locks views as their owner. This non-login
      -- role needs UPDATE for that lock; no UPDATE policy or caller grant exists.
      EXECUTE format('GRANT SELECT, UPDATE ON _oo_load.%I TO oo_demo_view_owner', stage);
    END LOOP;
    EXECUTE format('ALTER VIEW _oo_load.%I OWNER TO oo_demo_view_owner', g.relation_name);
  END LOOP;
END $policy$;
""")


def main(request):
    root = Path(request["root"])
    if request["action"] == "down" and not (root / "owner.json").exists():
        return dict(status="not_started", resources=[])
    deployment = Deployment(root)
    if request["action"] == "down":
        return deployment.down()
    if request["action"] == "audit":
        return deployment.collect_audit(request["queries"])
    if request["action"] != "up":
        raise ValueError("DEMO_ACTION")
    return deployment.deploy(Path(request["assets"]))


if __name__ == "__main__":
    try:
        print(json.dumps(main(json.load(sys.stdin))))
    except (OSError, ValueError, KeyError, subprocess.SubprocessError):
        print('{"status":"failed","error":"DEMO_DEPLOY_FAILED","publication":"not_claimed"}')
        sys.exit(1)
