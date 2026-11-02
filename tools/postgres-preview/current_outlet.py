#!/usr/bin/env python3
"""PG publication adapter for the existing Library Selfbuild, not a new builder."""
import argparse
from contextlib import contextmanager
import hashlib
import importlib
import json
import os
from pathlib import Path
import subprocess
import sys
import time


def require(ok, reason):
    if not ok:
        raise ValueError(reason)


def read(path):
    return json.loads(Path(path).read_text())


def canonical(value):
    return json.dumps(value, sort_keys=True, ensure_ascii=False, separators=(",", ":"))


def digest(value):
    return hashlib.sha256(canonical(value).encode()).hexdigest()


def pin(path):
    path = Path(path).resolve()
    return {"path": str(path), "sha256": hashlib.sha256(path.read_bytes()).hexdigest()}


def verified(reference):
    path = Path(reference["path"])
    require(path.is_absolute() and not path.is_symlink() and path.is_file(), "pg_outlet_artifact")
    require(pin(path)["sha256"] == reference["sha256"], "pg_outlet_artifact_drift")
    return path


def save(path, value):
    path = Path(path)
    data = canonical(value) + "\n"
    if path.exists():
        require(path.read_text() == data, "pg_outlet_existing_artifact_drift")
        return
    path.parent.mkdir(mode=0o700, parents=True, exist_ok=True)
    with path.open("x") as stream:
        os.chmod(path, 0o600)
        stream.write(data)


def literal(value):
    return "'" + str(value).replace("'", "''") + "'"


def ident(value):
    return '"' + value.replace('"', '""') + '"'


def metadata(state, expected_sha):
    """Adapt measured PG catalog fields, never invent defaults, comments or DDL."""
    shard = state["shard"]
    receipt = shard["receipt"]
    require(shard["receipt_sha256"] == expected_sha == digest(receipt), "pg_outlet_publication_digest")
    require(receipt["position_codec"] == "pg-commit-lsn/v1"
            and receipt["consistency"] == "transactional" and receipt["consistency_grade"] == "T"
            and receipt["cross_table_consistent"] is True, "pg_outlet_publication_kind")
    stage = state["stage"]
    require(receipt["pg_current"]["bundle_sha256"] == stage["bundle_sha"]
            and receipt["source_id"] == stage["source"]
            and receipt["pg_current"]["task_id"] == stage["task"]
            and receipt["pg_current"]["end_lsn"] == stage["boundary"]["end_lsn"],
            "pg_outlet_capture_binding")
    published = {(t["schema"], t["table"]): t for t in receipt["tables"]}
    require(len(published) == len(receipt["tables"]) == len(stage["tables"]), "pg_outlet_table_coverage")
    result = {}
    for table in stage["tables"]:
        key = (table["schema"], table["table"])
        require(key in published and ".".join(key) not in result, "pg_outlet_table_coverage")
        stored = published[key]
        source_columns = table["columns"]
        require(len({c["name"] for c in source_columns}) == len(source_columns), "pg_outlet_columns")
        mappings = [m for m in state["mappings"] if m["source"] == receipt["source_id"]
                    and m["logical_namespace"] == key[0] and m["logical_table"] == key[1]
                    and m["level"] == "c" and m["scope_table_physical"] == stored["physical_name"]]
        require(len(mappings) == len(source_columns)
                and {m["logical_column"] for m in mappings} == {c["name"] for c in source_columns}
                and all(m["config_digest"] == stage["bundle_sha"]
                        and m["run_generation"] == receipt["source_generation"] for m in mappings),
                "pg_outlet_mapping_binding")
        columns = {}
        for column in source_columns:
            require(column["oid"] in (20, 25) and column["typmod"] == -1
                    and type(column["nullable"]) is bool, "pg_outlet_bounded_type")
            mapped = next(m for m in mappings if m["logical_column"] == column["name"])
            attrs = [a for a in state["attributes"] if a["table"] == stored["physical_name"]
                     and a["column"] == mapped["physical_name"]]
            require(len(attrs) == 1 and attrs[0]["oid"] == column["oid"]
                    and attrs[0]["typmod"] == column["typmod"]
                    and attrs[0]["not_null"] == (not column["nullable"]), "pg_outlet_target_type")
            columns[column["name"]] = {
                "source_engine": "postgresql", "source_type": {20: "bigint", 25: "text"}[column["oid"]],
                "nullable": column["nullable"], "comment": "", "comment_state": "unmeasured",
                "source_oid": column["oid"], "source_typmod": column["typmod"],
                "default_observation": {"state": "unmeasured"},
                "source_catalog_sha256": digest(table),
                "pg_publication_sha256": expected_sha,
            }
        require(table["primary_key"] in columns
                and not columns[table["primary_key"]]["nullable"], "pg_outlet_primary_key")
        result[".".join(key)] = {
            "schema": key[0], "columns": columns, "primary_key": [table["primary_key"]],
            # Like the existing Oracle adapter, this legacy field hashes real
            # catalog input, not a fabricated CREATE TABLE or source attestation.
            "ddl_sha256": digest(table), "schema_basis": "admitted_pg_capture_catalog",
            "table_comment": "", "constraints_sql": [], "table_options_sql": None,
            "unmeasured": ["comments", "defaults", "non_primary_constraints", "table_options"],
        }
    require(set(result) == {s + "." + t for s, t in published}, "pg_outlet_table_coverage")
    return result


class Outlet:
    def __init__(self, operator):
        path = Path(operator)
        require(path.is_absolute() and not path.is_symlink()
                and not path.stat().st_mode & 0o077, "pg_outlet_private_operator")
        self.op = read(path)
        require(self.op["protocol"] == "oo.pg-current.outlet/v1", "pg_outlet_protocol")
        self.root = Path(self.op["root"])
        require(self.root.is_absolute() and not self.root.is_symlink()
                and not (self.root / ".git").exists(), "pg_outlet_private_root")
        helpers = read(verified(self.op["helper_pins"]))
        for name, sha in helpers["files"].items():
            require(not Path(name).is_absolute() and ".." not in Path(name).parts, "pg_outlet_helper_name")
            verified({"path": str(Path(helpers["path"]) / name), "sha256": sha})
        import_path = helpers.get("import_path", helpers["path"])
        require(Path(import_path).is_relative_to(helpers["path"]), "pg_outlet_helper_name")
        sys.path.insert(0, import_path)
        self.lib = importlib.import_module("library_selfbuild")
        self.build = importlib.import_module("library_build")
        self.control = importlib.import_module("library_control")
        require(Path(self.lib.__file__).parent == Path(import_path), "pg_outlet_helper_origin")
        self.pipeline = verified(self.op["pipeline"])
        self.db = self.lib.ReadOnlyPostgres(self.op["admin_psql"], 30000)
        self.source_metadata = None
        self.model_calls = 0

    def invoke(self, label, command, value):
        attempt = self.root / "attempts" / (str(time.time_ns()) + "-" + label)
        attempt.mkdir(mode=0o700, parents=True)
        save(attempt / "input.json", value)
        result = subprocess.run(list(map(str, command)), input=canonical(value), text=True,
                                capture_output=True, timeout=120)
        (attempt / "stdout").write_text(result.stdout)
        (attempt / "stderr").write_text(result.stderr)
        save(attempt / "command.json", {"argv": list(map(str, command)), "exit": result.returncode})
        require(result.returncode == 0, "pg_outlet_child_refused")
        body = json.loads(result.stdout)
        require(not body.get("error") and body.get("outcome") != "refused", "pg_outlet_child_refused")
        return body

    def current(self):
        source = literal(self.op["source_id"])
        state = self.db.query(
            "select jsonb_build_object('shard',_oo.native_library_c1_shard(" + source + "),"
            "'stage',(select to_jsonb(r) from _oo.pg_current_run r where source=" + source + "),"
            "'mappings',(select jsonb_agg(to_jsonb(m)) from _oo.physical_mappings m where source=" + source + "),"
            "'attributes',(select jsonb_agg(jsonb_build_object('table',c.relname,'column',a.attname,"
            "'oid',a.atttypid::int,'typmod',a.atttypmod,'not_null',a.attnotnull))"
            " from pg_class c join pg_namespace n on n.oid=c.relnamespace join pg_attribute a on a.attrelid=c.oid"
            " where n.nspname='_oo' and a.attnum>0 and not a.attisdropped"
            " and c.relname in (select physical_name from _oo.table_freshness where source=" + source + ")))")
        sources = metadata(state, self.op["publication_sha256"])
        require(sorted(sources) == sorted(self.op["datasets"]), "pg_outlet_operator_inventory")
        require(state["shard"]["receipt"]["source_id"] == self.op["source_id"], "pg_outlet_source")
        self.source_metadata = sources
        return state

    @contextmanager
    def adapter(self, allow_model=False):
        self.current()
        old_metadata, old_source_for = self.lib.source_metadata, self.lib.source_for
        old_validate, old_call = self.lib.validate_retained_tables, self.lib.call_model

        def source_metadata(ddl_dir, oracle_input=None, inventory=None):
            require(ddl_dir is None and oracle_input is None, "pg_outlet_ambiguous_adapter")
            return json.loads(canonical(self.source_metadata))

        def validate(db, tables, ddl_dir=None, *, oracle_input=None):
            self.current()
            return old_validate(db, tables, ddl_dir, oracle_input=oracle_input)

        def call_model(batch, config):
            require(allow_model and self.model_calls == 0, "pg_outlet_model_not_authorized")
            self.model_calls += 1
            try:
                return old_call(batch, config)
            except Exception as error:
                # This operator authorizes one actual attempt, not automatic
                # provider retries. Preserve the original error and raw archive.
                error.retryable = False
                error.stops_run = True
                raise

        self.lib.source_metadata = source_metadata
        self.lib.source_for = lambda sources, fresh, oracle_input: sources.get(
            fresh["schema_name"] + "." + fresh["table_name"], {})
        self.lib.validate_retained_tables = validate
        self.lib.call_model = call_model
        try:
            yield
        finally:
            self.lib.source_metadata, self.lib.source_for = old_metadata, old_source_for
            self.lib.validate_retained_tables, self.lib.call_model = old_validate, old_call

    def prepare(self):
        state = self.current()
        save(self.root / "published-input.json", state)
        save(self.root / "source-metadata.json", self.source_metadata)
        manifest = read(self.root / "manifest.json")
        prepared = self.invoke("native-library-prepare", [self.pipeline, "native-library-prepare"], {
            "protocol": "oo.pipeline.native-library-prepare/v1", "target": self.op["target"],
            "manifest": {"library_id": self.op["project"], "sources": manifest["sources"]}})
        require(prepared["precheck"] == "rolled_back"
                and prepared["native_mapping"]["publication"]["receipt"]["shards"] == [state["shard"]],
                "pg_outlet_native_mapping")
        save(self.root / "native-library-prepared.json", prepared)
        with self.adapter():
            result = self.build.run(self.root / "build-config.json", self.root / "build",
                                    through="observation", resume=(self.root / "build").exists())
        facts = read(self.root / "build/facts.json")
        self.lib.require_complete_facts(facts)
        require(sorted(t["dataset"] for t in facts["tables"]) == sorted(self.op["datasets"]), "pg_outlet_facts")
        publication = state["shard"]["receipt"]
        for table in facts["tables"]:
            sealed = next(t for t in publication["tables"] if t["schema"] + "." + t["table"] == table["dataset"])
            require(table["source_id"] == self.op["source_id"]
                    and table["source_generation"] == publication["source_generation"]
                    and table["target_generation"] == publication["target_generation"]
                    and all(c["profile"]["rows"] == sealed["accepted_rows"] for c in table["columns"]),
                    "pg_outlet_facts_binding")
        config = read(self.root / "build-config.json")
        batches = self.lib.configured_batches(facts, config)
        require(len(batches) == 1, "pg_outlet_single_model_batch")
        preflight = []
        for batch in batches:
            request, body = self.lib.draft_request(batch, config)
            path = self.root / "pre-S5" / ("batch-" + str(batch["index"]) + ".request.json")
            save(path, json.loads(body))
            # Keep the exact bytes that the real model adapter will send.
            raw_path = path.with_suffix(".http-body")
            if raw_path.exists():
                require(raw_path.read_bytes() == body, "pg_outlet_prompt_drift")
            else:
                raw_path.write_bytes(body)
            require(len(body) <= config.get("max_batch_context_bytes", 240000), "pg_outlet_prompt_budget")
            preflight.append({"request": pin(raw_path), "bytes": len(body), "tables": len(batch["tables"]),
                              "max_tokens": request["max_tokens"], "semantic_input_digest": batch["semantic_input_digest"]})
        save(self.root / "pre-S5/ready.json", {
            "protocol": "oo.pg-current.pre-s5/v1", "stages": ["S0", "S1", "S2", "S3", "S4"],
            "result": result, "facts": pin(self.root / "build/facts.json"),
            "tables": len(facts["tables"]), "columns": sum(len(t["columns"]) for t in facts["tables"]),
            "batches": preflight, "model_calls": 0, "source_contacted": False,
            "blocker": self.op["model_blocker"], "pack_built": False, "gateway_accepted": False})
        return read(self.root / "pre-S5/ready.json")

    def draft(self, allow_model):
        require(allow_model, "pg_outlet_model_not_authorized")
        require((self.root / "pre-S5/ready.json").is_file(), "pg_outlet_pre_s5_required")
        config = read(self.root / "build-config.json")
        require(config["s5_concurrency"] == config["max_draft_batches"] == 1, "pg_outlet_one_call_budget")
        with self.adapter(allow_model=True):
            return self.build.run(self.root / "build-config.json", self.root / "build", through="draft", resume=True)

    def package(self, review_file):
        require(review_file is not None and (self.root / "build/S5-receipt.json").is_file(),
                "pg_outlet_genuine_s5_and_review_required")
        # Normal pinned review interface; this harness never invents a reviewer.
        config = read(self.root / "build-config.json")
        config["column_review_input"] = pin(review_file)
        save(self.root / "package-config.json", config)
        with self.adapter():
            return self.build.run(self.root / "package-config.json", self.root / "build",
                                  through="package", resume=True)


def gold(operator):
    """Compute reviewable expected answers from retained source gold, not target guesses."""
    outlet = Outlet(operator)
    state = outlet.current()
    queries = []
    observer = outlet.lib.ReadOnlyPostgres(outlet.op["observer_psql"], 30000)
    for table in state["stage"]["tables"]:
        dataset = table["schema"] + "." + table["table"]
        source = verified(outlet.op["source_gold"][dataset])
        rows, non_null, total, minimum, maximum = 0, 0, 0, None, None
        revision_total = 0
        text_probe = None
        with source.open() as stream:
            for line in stream:
                row = json.loads(line)
                rows += 1
                key = row[table["primary_key"]]
                total += key
                minimum = key if minimum is None else min(minimum, key)
                maximum = key if maximum is None else max(maximum, key)
                non_null += row.get("value") is not None
                revision_total += row.get("revision", 0)
                if text_probe is None and isinstance(row.get("value"), str) and len(row["value"]) <= 80:
                    text_probe = (key, row["value"])
        logical = ident(table["schema"]) + "." + ident(table["table"])
        key = ident(table["primary_key"])
        expressions = [f"COUNT(*) AS row_count", f"SUM({key}) AS id_total", f"MIN({key}) AS id_min", f"MAX({key}) AS id_max"]
        expected = [rows, total if rows else None, minimum, maximum]
        if any(c["name"] == "revision" for c in table["columns"]):
            expressions.append("SUM(revision) AS revision_total")
            expected.append(revision_total if rows else None)
        sql = "SELECT " + ", ".join(expressions) + " FROM " + logical
        mapped = next(t for t in state["shard"]["receipt"]["tables"]
                      if t["schema"] + "." + t["table"] == dataset)
        mapping = {m["logical_column"]: m["physical_name"] for m in state["mappings"]
                   if m["scope_table_physical"] == mapped["physical_name"] and m["level"] == "c"}
        actual_expr = [f"count(*)", f"sum({ident(mapping[table['primary_key']])})",
                       f"min({ident(mapping[table['primary_key']])})", f"max({ident(mapping[table['primary_key']])})"]
        if "revision" in mapping:
            actual_expr.append("sum(" + ident(mapping["revision"]) + ")")
        actual = observer.query("select json_build_array(" + ",".join(actual_expr)
                                + ") from _oo." + ident(mapped["physical_name"]))
        require(actual == expected, "pg_outlet_gold_aggregate_difference")
        queries.append({"dataset": dataset, "sql": sql, "expected_rows": [expected],
                        "source_gold": outlet.op["source_gold"][dataset],
                        "empty_explicit": rows == 0, "target_read_only_verified": True})
        if text_probe:
            k, text = text_probe
            sql = ("SELECT " + key + ", " + ident("value") + " FROM " + logical
                   + " WHERE " + key + "=" + str(k))
            actual = observer.query("select json_agg(json_build_array(" + ident(mapping[table["primary_key"]])
                                    + "," + ident(mapping["value"]) + ")) from _oo." + ident(mapped["physical_name"])
                                    + " where " + ident(mapping[table["primary_key"]]) + "=" + str(k))
            require(actual == [[k, text]], "pg_outlet_gold_text_difference")
            queries.append({"dataset": dataset, "sql": sql, "expected_rows": [[k, text]],
                            "value_sensitive": True, "source_gold": outlet.op["source_gold"][dataset],
                            "target_read_only_verified": True})
    result = {"protocol": "oo.pg-current.outlet-gold/v1", "queries": queries,
              "source_contacted": False, "gateway_executed": False, "counts_only": False}
    save(outlet.root / "gold-queries.json", result)
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("action", choices=["prepare", "gold", "draft", "package"])
    parser.add_argument("--operator", required=True)
    parser.add_argument("--allow-one-model-call", action="store_true",
                        help="Main-only explicit authorization AFTER provider recovery; never a readiness probe.")
    parser.add_argument("--column-review")
    args = parser.parse_args()
    os.umask(0o077)
    require(not args.allow_one_model_call or args.action == "draft", "pg_outlet_model_flag")
    try:
        if args.action == "gold":
            result = gold(args.operator)
        else:
            outlet = Outlet(args.operator)
            if args.action == "prepare":
                result = outlet.prepare()
            elif args.action == "draft":
                result = outlet.draft(args.allow_one_model_call)
            else:
                result = outlet.package(args.column_review)
        print(canonical(result))
    except Exception as error:
        reason = str(error)
        if not reason.replace("_", "").isalnum() or len(reason) > 100:
            reason = type(error).__name__
        print(canonical({"outcome": "refused", "reason": reason, "published": False}))
        raise SystemExit(2) from None


if __name__ == "__main__":
    main()
