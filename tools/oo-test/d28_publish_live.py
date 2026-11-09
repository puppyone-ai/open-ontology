"""PG17 retained draft/adjudicate/confirm/compile/publish, no model network."""
import argparse
import copy
import importlib.util
import json
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[2]
os.environ["OO_TEST_LANE"] = "g"
sys.path.insert(0, str(ROOT / "tools/pg-builder/scripts"))
sys.path.insert(0, str(ROOT / "tools/release-coordinator/tests"))
import adjudication
import library_selfbuild as lib
import retained_build
import retained_cycle_live as live
import selfbuild_budget
from test_selfbuild_budget import Acceptance, Model


class Confirmed(live.Retained):
    def __init__(self, out):
        super().__init__(out, "postgres", None)

    def start_gateway(self):
        # This card proves publisher activation, not Gateway/business acceptance.
        pass

    def load(self, prefix, run):
        (self.db.directory / "source-ca").chmod(0o600)
        return super().load(prefix, run)

    def publish(self, run):
        cut = self.prepare(run)
        self.cuts.append(cut)
        self.db.sql("CREATE ROLE oo_r_owner NOLOGIN NOINHERIT; "
                    "GRANT oo_r_owner TO oo_f22_login WITH INHERIT FALSE, SET TRUE; "
                    "CREATE ROLE d28_reader LOGIN; "
                    "GRANT USAGE ON SCHEMA _oo,_oo_load TO d28_reader; "
                    "GRANT SELECT ON ALL TABLES IN SCHEMA _oo,_oo_load TO d28_reader;")
        db = lib.ReadOnlyPostgres(
            ["docker", "exec", "-i", self.db.name, "psql", "-U", "postgres", "-d", "postgres",
             "-c", "SET ROLE d28_reader", "-f", "-"],
            120000)
        config = dict(library_id="owned", generation=run, source_engine="postgres")
        facts, provenance = retained_build.facts_for(db, config)
        tables = retained_build.inventory(facts)
        draft = selfbuild_budget.Runner(
            Acceptance().config(), self.out / "replay-draft", Model()).run(tables, [])
        command = retained_build.review_input(tables, draft, "owned")
        command["adjudication_replica"] = provenance
        adjudication.discover_candidates(command, facts["tables"])
        raw = lib.canonical(command)
        document = adjudication.collect(db, command, facts["tables"], live.sha(raw.encode()))
        overrides = dict(protocol="oo.adjudication-override/v1",
                         summary_sha256=document["summary_sha256"], items={})
        confirmation = dict(protocol="oo.publish-confirm/v1", summary_sha256=document["summary_sha256"],
                            overrides_sha256=None, reason="Owned fixture owner confirmation",
                            confirmed_at="2026-11-09T12:00:00Z")
        envelope = dict(adjudication_input=raw, adjudication=document,
                        overrides=overrides, confirmation=confirmation)

        def compile_envelope(value, label):
            result = subprocess.run([str(live.BIN / "selfbuild-budget")],
                input=lib.canonical(dict(operation="reviewed", command=value)),
                text=True, capture_output=True)
            (self.out / (label + ".stdout")).write_text(result.stdout)
            (self.out / (label + ".stderr")).write_text(result.stderr)
            return result

        result = compile_envelope(envelope, "compile")
        assert result.returncode == 0, result.stderr
        compiled = json.loads(result.stdout)
        reviewed = dict(protocol="oo.owner-reviewed-registry/v1", status="reviewed",
                        published=False, authentication="not_claimed", command=envelope,
                        compiled=compiled, review_decision=confirmation)
        changed = copy.deepcopy(reviewed)
        changed["command"]["overrides"]["items"]["entity_key/app.items"] = dict(
            decision="rejected", reason="Keep fixture as a plain dataset")
        stale = compile_envelope(changed["command"], "stale-confirmation")
        assert stale.returncode != 0 and "adjudication_confirmation_binding" in stale.stdout + stale.stderr
        self.refuse("stale-confirmation", run, lib.canonical(changed).encode(), "review_compile_refused")
        effective = copy.deepcopy(document)
        next(i for i in effective["items"] if i["class"] == "entity_key")["decision"] = "rejected"
        effective["graph"]["entities"][0].update(key=[], display=None)
        changed_hash = adjudication.summary_hash(effective)
        assert changed_hash != document["summary_sha256"]
        changed["command"]["confirmation"].update(
            summary_sha256=changed_hash,
            overrides_sha256=lib.digest(changed["command"]["overrides"]))
        changed["review_decision"] = changed["command"]["confirmation"]
        fresh = compile_envelope(changed["command"], "override-confirmed")
        assert fresh.returncode == 0, fresh.stderr
        changed["compiled"] = json.loads(fresh.stdout)
        assert changed["compiled"]["summary_sha256"] == changed_hash
        tampered = copy.deepcopy(reviewed)
        tampered["compiled"]["producer"]["pack_digest"] = "0" * 64
        self.refuse("compiled-tamper", run, lib.canonical(tampered).encode(), "review_compiled_mismatch")
        code, receipt = self.publisher(run, lib.canonical(changed).encode(), "confirmed")
        assert code == 0 and receipt["outcome"] == "activated" and receipt["state"] == "ready", receipt
        for name, value in [("draft", draft), ("adjudication", document), ("reviewed", changed)]:
            live.cycles.write_json(self.out / (name + ".json"), value)
        self.results.update(version=self.db.sql("SHOW server_version").strip(),
                            model="deterministic replay; no external model",
                            original_hash=document["summary_sha256"], override_hash=changed_hash,
                            publisher=receipt, state=self.state())
        live.cycles.write_json(self.out / "result.json", self.results)
        return receipt


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    args.output.mkdir(parents=True, exist_ok=False)
    fixture = Confirmed(args.output.resolve())
    try:
        fixture.start()
    finally:
        fixture.close()


if __name__ == "__main__":
    main()
