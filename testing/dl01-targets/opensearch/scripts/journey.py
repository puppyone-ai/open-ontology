#!/usr/bin/env python3
"""Read-only OpenSearch PIT/search_after journey against the owned synthetic index.

The typed oracle is testing/dl01-targets/opensearch/oracle/expected.json.
This process does not write that file. Setup uses the DBA identity. The reader
identity is the only business connection.
"""

from __future__ import annotations

import base64
import json
import ssl
import subprocess
import threading
import time
from http.client import HTTPSConnection
from pathlib import Path

ROOT = Path("/Users/redux/puppy-one/open-ontology/testing/dl01-targets/opensearch")
EVIDENCE = Path(
    "/Users/redux/puppy-one/open-ontology/evidence/b53c75ff-62f4-4869-8428-a605d263cb43/DL01/opensearch/raw/journey"
)
PRIVATE = Path("/tmp/oo-dl01w1-15-private")
INDEX = "oo-dl01w1-15-synthetic"
OTHER = "oo-dl01w1-15-other"
PORT = 19215
CONTAINER = "oo-dl01w1-15-opensearch"
EXPECTED_VERSION = "3.9.0"


def load_identities() -> dict[str, str]:
    data: dict[str, str] = {}
    for line in (PRIVATE / "identities.env").read_text(encoding="utf-8").splitlines():
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, value = line.split("=", 1)
        data[key] = value
    return data


class Api:
    def __init__(self, ca: Path, user: str = "", password: str = "", cert: Path | None = None, key: Path | None = None):
        self.user = user
        self.password = password
        self.ctx = ssl.create_default_context(cafile=str(ca))
        self.ctx.minimum_version = ssl.TLSVersion.TLSv1_2
        if cert is not None and key is not None:
            self.ctx.load_cert_chain(str(cert), str(key))
        self.conn: HTTPSConnection | None = None

    def close(self) -> None:
        if self.conn is not None:
            self.conn.close()
            self.conn = None

    def call(self, method: str, path: str, body=None, content_type: str = "application/json", timeout: int = 60):
        data = None
        headers = {"Accept": "application/json"}
        if self.user:
            token = base64.b64encode(f"{self.user}:{self.password}".encode("utf-8")).decode("ascii")
            headers["Authorization"] = "Basic " + token
        if body is not None:
            if isinstance(body, (dict, list)):
                data = json.dumps(body, ensure_ascii=False, allow_nan=False).encode("utf-8")
            elif isinstance(body, str):
                data = body.encode("utf-8")
            else:
                data = body
            headers["Content-Type"] = content_type
        last_error = None
        for _ in range(2):
            try:
                if self.conn is None:
                    self.conn = HTTPSConnection("127.0.0.1", PORT, context=self.ctx, timeout=timeout)
                self.conn.request(method, path, body=data, headers=headers)
                resp = self.conn.getresponse()
                raw = resp.read()
                return resp.status, raw
            except Exception as exc:  # connection reset during startup or cancel
                last_error = exc
                self.close()
                time.sleep(0.2)
        raise RuntimeError(f"{method} {path} failed") from last_error


def parse_json(raw: bytes):
    if not raw:
        return None
    return json.loads(raw.decode("utf-8"))


def secrets_of(ident: dict[str, str]) -> list[str]:
    values = [ident.get("dba_password", ""), ident.get("reader_password", "")]
    users = PRIVATE / "security" / "internal_users.yml"
    if users.exists():
        for line in users.read_text(encoding="utf-8").splitlines():
            if "hash:" in line:
                values.append(line.split(":", 1)[1].strip().strip('"'))
    return [value for value in values if len(value) >= 8]


def redact(text: str, secrets: list[str]) -> str:
    for secret in secrets:
        text = text.replace(secret, "[redacted]")
    return text


def write_phase(name: str, payload, exit_code: int, secrets: list[str]) -> None:
    EVIDENCE.mkdir(parents=True, exist_ok=True)
    text = payload if isinstance(payload, str) else json.dumps(payload, indent=2, ensure_ascii=False, default=str)
    (EVIDENCE / f"{name}.json").write_text(redact(text, secrets) + "\n", encoding="utf-8")
    (EVIDENCE / f"{name}.exit").write_text(f"{exit_code}\n", encoding="utf-8")


def same(left, right) -> bool:
    if type(left) is not type(right):
        return False
    if isinstance(left, dict):
        if left.keys() != right.keys():
            return False
        return all(same(left[key], right[key]) for key in left)
    if isinstance(left, list):
        return len(left) == len(right) and all(same(a, b) for a, b in zip(left, right))
    return left == right


def task_ids(payload) -> list[str]:
    found: list[str] = []
    if not isinstance(payload, dict):
        return found
    tasks = payload.get("tasks")
    if isinstance(tasks, dict):
        for task_id, task in tasks.items():
            if isinstance(task, dict) and "action" in task:
                if "search" in str(task.get("action", "")):
                    found.append(task_id)
        if found:
            return found
    nodes = payload.get("nodes") or {}
    if isinstance(nodes, dict):
        for node in nodes.values():
            if not isinstance(node, dict):
                continue
            for task_id, task in (node.get("tasks") or {}).items():
                if "search" in str((task or {}).get("action", "")):
                    found.append(task_id)
    return found


def presence(source: dict, field: str) -> str:
    if field not in source:
        return "missing"
    value = source[field]
    if value is None:
        return "null"
    if value == "":
        return "empty"
    if isinstance(value, list):
        return "multivalued"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, (int, float)):
        return "numeric"
    if isinstance(value, str):
        return "text"
    return type(value).__name__


def container_state() -> str:
    proc = subprocess.run(
        [
            "docker", "inspect", "-f",
            "{{.State.Status}} exit={{.State.ExitCode}} oom={{.State.OOMKilled}} error={{.State.Error}}",
            CONTAINER,
        ],
        text=True,
        capture_output=True,
    )
    if proc.returncode != 0:
        return f"inspect_failed {proc.stderr.strip()[:200]}"
    return proc.stdout.strip()


def wait_ready(dba: Api, secrets: list[str]) -> dict:
    deadline = time.time() + 180
    last = None
    while time.time() < deadline:
        state = container_state()
        if state.startswith("exited") or state.startswith("dead") or state.startswith("inspect_failed"):
            last = {"error": "container_not_running", "state": state}
            break
        try:
            status, raw = dba.call("GET", "/")
            last = {"status": status, "body": raw.decode("utf-8", "replace")[:2000], "state": state}
            if status == 200:
                body = parse_json(raw)
                write_phase("05-ready", body, 0, secrets)
                return body
        except Exception as exc:
            cause = exc.__cause__ or exc
            last = {
                "error": type(exc).__name__,
                "cause": type(cause).__name__,
                "message": str(cause)[:400],
                "state": state,
            }
        time.sleep(2)
    write_phase("05-ready", last, 1, secrets)
    raise SystemExit(1)


def main() -> None:
    ident = load_identities()
    secrets = secrets_of(ident)
    gold = json.loads((ROOT / "oracle" / "expected.json").read_text(encoding="utf-8"))
    mapping = json.loads((ROOT / "oracle" / "mapping.json").read_text(encoding="utf-8"))
    ca = PRIVATE / "certs" / "ca.pem"
    dba = Api(ca, ident["dba_user"], ident["dba_password"])
    reader = Api(ca, ident["reader_user"], ident["reader_password"])
    try:
        info = wait_ready(dba, secrets)
        version = info.get("version", {})
        identity = {
            "distribution": version.get("distribution"),
            "number": version.get("number"),
            "tagline": info.get("tagline"),
            "cluster_name": info.get("cluster_name"),
            "lucene_version": version.get("lucene_version"),
            "build_hash": version.get("build_hash"),
        }
        reader_status, reader_raw = reader.call("GET", "/")
        reader_info = parse_json(reader_raw) if reader_status == 200 else {}
        dba_auth_status, dba_auth_raw = dba.call("GET", "/_plugins/_security/authinfo")
        reader_auth_status, reader_auth_raw = reader.call("GET", "/_plugins/_security/authinfo")
        identity.update(
            {
                "reader_root_status": reader_status,
                "reader_root_body": None if reader_status == 200 else reader_raw.decode("utf-8", "replace")[:500],
                "reader_distribution": (reader_info or {}).get("version", {}).get("distribution"),
                "dba_auth_status": dba_auth_status,
                "dba_auth_body": None if dba_auth_status == 200 else dba_auth_raw.decode("utf-8", "replace")[:500],
                "dba_user": (parse_json(dba_auth_raw) or {}).get("user_name") if dba_auth_status == 200 else None,
                "reader_auth_status": reader_auth_status,
                "reader_auth_body": None if reader_auth_status == 200 else reader_auth_raw.decode("utf-8", "replace")[:500],
                "reader_user": (parse_json(reader_auth_raw) or {}).get("user_name") if reader_auth_status == 200 else None,
            }
        )
        # The reader role has no cluster permissions. GET / and authinfo may be 403.
        # Product identity comes from the setup connection. The reader is proven by
        # a later index read and by write denial.
        reader_root_ok = reader_status == 200 and identity["reader_distribution"] == "opensearch"
        reader_auth_ok = identity["reader_user"] == ident["reader_user"]
        identity_ok = (
            identity["distribution"] == "opensearch"
            and identity["number"] == EXPECTED_VERSION
            and "OpenSearch" in (identity["tagline"] or "")
            and identity["dba_user"] == ident["dba_user"]
            and identity["dba_user"] != ident["reader_user"]
            and (reader_root_ok or reader_status == 403)
            and (reader_auth_ok or reader_auth_status == 403)
            and reader_status != 401
            and reader_auth_status != 401
        )
        write_phase("06-identity", identity, 0 if identity_ok else 1, secrets)
        if not identity_ok:
            raise SystemExit(1)

        created_status, created_raw = dba.call(
            "PUT",
            f"/{INDEX}",
            {"settings": {"index": {"number_of_shards": 1, "number_of_replicas": 0}}, "mappings": mapping},
        )
        lines = []
        for doc in gold["documents"]:
            lines.append(json.dumps({"index": {"_id": doc["doc_key"]}}, ensure_ascii=False))
            lines.append(json.dumps(doc["source"], ensure_ascii=False, allow_nan=False))
        bulk_status, bulk_raw = dba.call(
            "POST",
            f"/{INDEX}/_bulk?refresh=true",
            "\n".join(lines) + "\n",
            content_type="application/x-ndjson",
        )
        bulk = parse_json(bulk_raw)
        setup = {
            "create_status": created_status,
            "create_error": None if created_status in (200, 201) else created_raw.decode("utf-8", "replace")[:1500],
            "bulk_status": bulk_status,
            "bulk_errors": None if not bulk else bulk.get("errors"),
            "bulk_items": 0 if not bulk else len(bulk.get("items", [])),
        }
        if bulk and bulk.get("errors"):
            setup["bulk_failures"] = [
                item for item in bulk.get("items", [])
                if list(item.values())[0].get("error")
            ][:6]
        write_phase("07-setup", setup, 0 if created_status in (200, 201) and bulk_status == 200 and not setup["bulk_errors"] else 1, secrets)
        if created_status not in (200, 201) or bulk_status != 200 or setup["bulk_errors"]:
            raise SystemExit(1)

        map_status, map_raw = reader.call("GET", f"/{INDEX}/_mapping")
        actual_map = parse_json(map_raw)
        props = (
            ((actual_map or {}).get(INDEX) or {}).get("mappings", {}).get("properties", {})
        )
        map_checks = {
            "status": map_status,
            "dynamic": ((actual_map or {}).get(INDEX) or {}).get("mappings", {}).get("dynamic"),
            "long_value": (props.get("long_value") or {}).get("type"),
            "double_value": (props.get("double_value") or {}).get("type"),
            "scaled_value": (props.get("scaled_value") or {}).get("type"),
            "tags": (props.get("tags") or {}).get("type"),
            "text_value": (props.get("text_value") or {}).get("type"),
            "doc_key": (props.get("doc_key") or {}).get("type"),
        }
        map_ok = (
            map_status == 200
            and map_checks["dynamic"] == "strict"
            and map_checks["long_value"] == "long"
            and map_checks["double_value"] == "double"
            and map_checks["scaled_value"] == "scaled_float"
            and map_checks["tags"] == "keyword"
            and map_checks["text_value"] == "text"
            and map_checks["doc_key"] == "keyword"
        )
        write_phase("08-mapping", map_checks, 0 if map_ok else 1, secrets)
        if not map_ok:
            raise SystemExit(1)

        pit_status, pit_raw = reader.call("POST", f"/{INDEX}/_search/point_in_time?keep_alive=2m")
        pit = parse_json(pit_raw) or {}
        pit_id = pit.get("pit_id")
        if pit_status != 200 or not pit_id:
            write_phase("09-batch", {"pit_status": pit_status, "body": pit_raw.decode("utf-8", "replace")[:1500]}, 1, secrets)
            raise SystemExit(1)
        pages = []
        search_after = None
        seen = []
        current = pit_id
        for page_no in range(1, 6):
            body = {
                "size": gold["page_size"],
                "track_total_hits": True,
                "query": {"match_all": {}},
                "sort": [{gold["sort_field"]: "asc"}],
                "pit": {"id": current, "keep_alive": "2m"},
            }
            if search_after is not None:
                body["search_after"] = search_after
            status, raw = reader.call("POST", "/_search", body)
            page = parse_json(raw) or {}
            hits = ((page.get("hits") or {}).get("hits")) or []
            pages.append(
                {
                    "page": page_no,
                    "status": status,
                    "hit_count": len(hits),
                    "sorts": [hit.get("sort") for hit in hits],
                    "total": ((page.get("hits") or {}).get("total") or {}).get("value"),
                }
            )
            if status != 200:
                pages[-1]["body"] = raw.decode("utf-8", "replace")[:1500]
                break
            if page.get("pit_id"):
                current = page["pit_id"]
            if not hits:
                break
            for hit in hits:
                seen.append({"doc_key": hit.get("_id"), "source": hit.get("_source"), "sort": hit.get("sort")})
            search_after = hits[-1].get("sort")
            total = pages[-1]["total"]
            if len(hits) < gold["page_size"] or (total is not None and len(seen) >= total):
                break
        mismatches = []
        by_key = {row["doc_key"]: row["source"] for row in seen}
        for doc in gold["documents"]:
            actual = by_key.get(doc["doc_key"])
            if actual is None or not same(actual, doc["source"]):
                mismatches.append({"doc_key": doc["doc_key"], "expected": doc["source"], "actual": actual})
        field_mismatches = []
        for item in gold["field_oracle"]:
            actual_source = by_key.get(item["doc_key"]) or {}
            got = presence(actual_source, item["field"])
            if got != item["kind"]:
                field_mismatches.append({**item, "actual_kind": got, "actual_value": actual_source.get(item["field"], "<missing>")})
        order = [row["doc_key"] for row in seen]
        expected_order = [doc["doc_key"] for doc in gold["documents"]]
        batch_ok = (
            not mismatches
            and not field_mismatches
            and order == expected_order
            and len(pages) == 3
            and all(page["hit_count"] == 4 and page["status"] == 200 for page in pages)
        )
        batch_report = {
            "opened_pit": True,
            "pages": pages,
            "order": order,
            "source_mismatches": mismatches,
            "field_mismatches": field_mismatches,
            "pit_id_changed": current != pit_id,
        }
        write_phase("09-batch", batch_report, 0 if batch_ok else 1, secrets)
        if not batch_ok:
            raise SystemExit(1)

        late = {"doc_key": "d99", "case_name": "after-pit", "text_value": "late"}
        put_status, _ = dba.call("PUT", f"/{INDEX}/_doc/d99?refresh=true", late)
        pit_body = {
            "size": 20,
            "query": {"match_all": {}},
            "sort": [{"doc_key": "asc"}],
            "pit": {"id": current, "keep_alive": "2m"},
        }
        pit_search_status, pit_search_raw = reader.call("POST", "/_search", pit_body)
        pit_hits = (((parse_json(pit_search_raw) or {}).get("hits") or {}).get("hits")) or []
        pit_keys = [hit.get("_id") for hit in pit_hits]
        if (parse_json(pit_search_raw) or {}).get("pit_id"):
            current = parse_json(pit_search_raw)["pit_id"]
        fresh_status, fresh_raw = reader.call(
            "POST",
            f"/{INDEX}/_search",
            {"size": 20, "query": {"match_all": {}}, "sort": [{"doc_key": "asc"}]},
        )
        fresh_keys = [hit.get("_id") for hit in ((((parse_json(fresh_raw) or {}).get("hits") or {}).get("hits")) or [])]
        delete_status, _ = dba.call("DELETE", f"/{INDEX}/_doc/d99?refresh=true")
        stable = {
            "dba_put_late_status": put_status,
            "pit_status": pit_search_status,
            "pit_keys": pit_keys,
            "fresh_status": fresh_status,
            "fresh_keys": fresh_keys,
            "dba_delete_late_status": delete_status,
            "pit_hides_late": "d99" not in pit_keys and len(pit_keys) == 12,
            "fresh_shows_late": "d99" in fresh_keys and len(fresh_keys) == 13,
        }
        stable_ok = put_status in (200, 201) and stable["pit_hides_late"] and stable["fresh_shows_late"] and delete_status == 200
        write_phase("10-pit-stability", stable, 0 if stable_ok else 1, secrets)
        if not stable_ok:
            raise SystemExit(1)

        other_status, _ = dba.call("PUT", f"/{OTHER}", {"mappings": {"dynamic": "strict", "properties": {"doc_key": {"type": "keyword"}}}})
        dba.call("PUT", f"/{OTHER}/_doc/x?refresh=true", {"doc_key": "x"})
        negatives = []

        def expect(name: str, status: int, raw: bytes, allowed: set[int]) -> None:
            negatives.append(
                {
                    "name": name,
                    "status": status,
                    "ok": status in allowed,
                    "body": raw.decode("utf-8", "replace")[:500],
                }
            )

        anon = Api(ca)
        expect("anonymous", *anon.call("GET", "/"), {401})
        anon.close()
        bad = Api(ca, ident["reader_user"], "not-the-password")
        expect("wrong_password", *bad.call("GET", "/"), {401})
        bad.close()
        expect("reader_put", *reader.call("PUT", f"/{INDEX}/_doc/nope", {"doc_key": "nope", "case_name": "denied"}), {403})
        expect("reader_delete", *reader.call("DELETE", f"/{INDEX}/_doc/d01"), {403})
        expect("reader_create_index", *reader.call("PUT", "/oo-dl01w1-15-forbidden", {"settings": {"index": {"number_of_shards": 1}}}), {403})
        expect("reader_other_mapping", *reader.call("GET", f"/{OTHER}/_mapping"), {403})
        expect("reader_other_search", *reader.call("POST", f"/{OTHER}/_search", {"query": {"match_all": {}}}), {403})
        expect("reader_list_pits", *reader.call("GET", "/_search/point_in_time/_all"), {403})
        expect("reader_delete_all_pits", *reader.call("DELETE", "/_search/point_in_time/_all"), {403})
        expect("reader_cat_indices", *reader.call("GET", "/_cat/indices?format=json"), {403})
        bad_doc_status, bad_doc_raw = dba.call(
            "PUT",
            f"/{INDEX}/_doc/bad-type",
            {"doc_key": "bad-type", "case_name": "bad", "long_value": "not-a-long"},
        )
        expect("string_into_long", bad_doc_status, bad_doc_raw, {400})
        unmapped_status, unmapped_raw = dba.call(
            "PUT",
            f"/{INDEX}/_doc/bad-field",
            {"doc_key": "bad-field", "case_name": "bad", "not_in_mapping": 1},
        )
        expect("unmapped_field", unmapped_status, unmapped_raw, {400})
        trust_error = None
        try:
            public_ctx = ssl.create_default_context()
            conn = HTTPSConnection("127.0.0.1", PORT, context=public_ctx, timeout=10)
            conn.request("GET", "/")
            conn.getresponse().read()
            trust_error = "unexpected_success"
        except Exception as exc:
            trust_error = type(exc).__name__
        negatives.append({"name": "public_ca_rejected", "ok": trust_error != "unexpected_success", "error": trust_error})
        neg_ok = other_status in (200, 201) and all(item["ok"] for item in negatives)
        write_phase("11-negative", {"other_index_status": other_status, "cases": negatives}, 0 if neg_ok else 1, secrets)
        if not neg_ok:
            raise SystemExit(1)

        slow_result: dict = {}

        def slow_search() -> None:
            try:
                status, raw = reader.call(
                    "POST",
                    f"/{INDEX}/_search",
                    {
                        "size": 1,
                        "query": {
                            "script_score": {
                                "query": {"match_all": {}},
                                "script": {
                                    "lang": "painless",
                                    "source": "long s = 0; for (long i = 0L; i < 80000000L; ++i) { s += (i & 7L); } return s;"
                                },
                            }
                        },
                    },
                    timeout=120,
                )
                slow_result["status"] = status
                slow_result["body"] = raw.decode("utf-8", "replace")[:1500]
            except Exception as exc:
                slow_result["error"] = type(exc).__name__

        thread = threading.Thread(target=slow_search)
        thread.start()
        cancelled = None
        seen_task = None
        deadline = time.time() + 20
        while time.time() < deadline and thread.is_alive():
            status, raw = dba.call("GET", "/_tasks?detailed=true&actions=*search")
            tasks = parse_json(raw) or {}
            running = task_ids(tasks)
            if running:
                seen_task = running[0]
                cancel_status, cancel_raw = dba.call("POST", f"/_tasks/{seen_task}/_cancel")
                cancelled = {
                    "task": seen_task,
                    "status": cancel_status,
                    "body": cancel_raw.decode("utf-8", "replace")[:800],
                }
                break
            time.sleep(0.2)
        thread.join(timeout=30)
        remain_status, remain_raw = dba.call("GET", "/_tasks?detailed=true&actions=*search")
        remain = parse_json(remain_raw) or {}
        remain_ids = task_ids(remain)
        cancel_report = {
            "saw_task": seen_task,
            "cancel": cancelled,
            "slow": {key: value for key, value in slow_result.items()},
            "remaining_search_tasks": remain_ids,
            "remain_status": remain_status,
        }
        slow_status = slow_result.get("status")
        cancel_ok = (
            seen_task is not None
            and cancelled is not None
            and cancelled["status"] == 200
            and seen_task not in remain_ids
            and slow_status != 200
        )
        write_phase("12-cancel", cancel_report, 0 if cancel_ok else 1, secrets)
        if not cancel_ok:
            raise SystemExit(1)

        delete_pit_status, delete_pit_raw = reader.call("DELETE", "/_search/point_in_time", {"pit_id": [current]})
        reuse_status, reuse_raw = reader.call(
            "POST",
            "/_search",
            {
                "size": 1,
                "query": {"match_all": {}},
                "sort": [{"doc_key": "asc"}],
                "pit": {"id": current, "keep_alive": "1m"},
            },
        )
        list_status, list_raw = dba.call("GET", "/_search/point_in_time/_all")
        listed = parse_json(list_raw) or {}
        listed_ids = [item.get("pit_id") for item in listed.get("pits", [])] if isinstance(listed, dict) else []
        cleanup = {
            "delete_status": delete_pit_status,
            "delete_body": delete_pit_raw.decode("utf-8", "replace")[:800],
            "reuse_status": reuse_status,
            "reuse_body": reuse_raw.decode("utf-8", "replace")[:800],
            "list_status": list_status,
            "pit_still_listed": current in listed_ids,
        }
        cleanup_ok = delete_pit_status == 200 and reuse_status >= 400 and current not in listed_ids
        write_phase("13-pit-cleanup", cleanup, 0 if cleanup_ok else 1, secrets)
        if not cleanup_ok:
            raise SystemExit(1)

        dba.call("POST", "/security-auditlog-*/_refresh")
        audit_status, audit_raw = dba.call(
            "POST",
            "/security-auditlog-*/_search",
            {"size": 300, "query": {"query_string": {"query": "oo_dl01w1_15_reader"}}},
        )
        audit_via = "dba"
        if audit_status != 200:
            admin = Api(
                ca,
                cert=PRIVATE / "certs" / "admin.pem",
                key=PRIVATE / "certs" / "admin-key.pem",
            )
            audit_status, audit_raw = admin.call(
                "POST",
                "/security-auditlog-*/_search",
                {"size": 300, "query": {"query_string": {"query": "oo_dl01w1_15_reader"}}},
            )
            audit_via = "admin_cert"
            admin.close()
        audit_body = parse_json(audit_raw) if audit_status == 200 else {"raw": audit_raw.decode("utf-8", "replace")[:2000]}
        hits = (((audit_body or {}).get("hits") or {}).get("hits")) or []
        paths = []
        for hit in hits:
            source = hit.get("_source") or {}
            paths.append(
                {
                    "category": source.get("audit_category") or source.get("audit_request_layer"),
                    "path": source.get("audit_rest_request_path") or source.get("audit_request_path"),
                    "user": source.get("audit_request_effective_user") or source.get("audit_request_initiating_user"),
                    "privilege": source.get("audit_request_privilege"),
                    "status": source.get("audit_request_status") or source.get("audit_rest_request_status"),
                }
            )
        reader_paths = [item.get("path") or "" for item in paths]
        audit_report = {
            "via": audit_via,
            "status": audit_status,
            "hit_count": len(hits),
            "events": paths[:80],
        }
        audit_ok = audit_status == 200 and any("_search" in path for path in reader_paths)
        write_phase("14-audit", audit_report, 0 if audit_ok else 1, secrets)
        if not audit_ok:
            raise SystemExit(1)
        write_phase("15-journey", {"ok": True}, 0, secrets)
    finally:
        reader.close()
        dba.close()


if __name__ == "__main__":
    main()
