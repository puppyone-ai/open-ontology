#!/usr/bin/env python3
"""Judge independent probe outputs. Does not rerun the checker."""

from __future__ import annotations

import json
from pathlib import Path

EV = Path("/Users/redux/puppy-one/open-ontology/execution/reviews/dl01-nightly-coordinator-fixed/linux")
PROBES = EV / "probes"


def load(name: str):
    exit_path = PROBES / f"{name}.exit"
    if not exit_path.exists():
        return None, None, f"missing {name}"
    code = int(exit_path.read_text().strip())
    raw = (PROBES / f"{name}.stdout").read_text() if (PROBES / f"{name}.stdout").exists() else ""
    data = None
    if raw.strip().startswith("{"):
        data = json.loads(raw)
    return code, data, raw


def errors(data: dict) -> list[str]:
    return [item["code"] for item in data["findings"] if item["severity"] == "error"]


def cell(data: dict, target: str) -> dict:
    rows = [item for item in data["cells"] if item["target_id"] == target]
    if len(rows) != 1:
        raise AssertionError(f"{target} cells={len(rows)}")
    return rows[0]


def nightly(data: dict, target: str) -> dict:
    rows = [item for item in data["nightly"] if item["target_id"] == target]
    if len(rows) != 1:
        raise AssertionError(f"{target} nightly={len(rows)}")
    return rows[0]


def main() -> None:
    failures: list[str] = []
    notes: list[str] = []

    def check(name: str, ok: bool, detail: str) -> None:
        line = f"{'pass' if ok else 'fail'} {name}: {detail}"
        notes.append(line)
        if not ok:
            failures.append(line)

    def expect_deny(name: str, target: str, required: set[str], forbidden: set[str] | None = None) -> None:
        code, data, _raw = load(name)
        if data is None:
            check(name, False, f"exit={code} not json")
            return
        got = set(errors(data))
        row = cell(data, target)
        denied = (
            code == 1
            and data["ok"] is False
            and data["promoted"] is False
            and data["matrix_rewritten"] is False
            and row["status"] == "failed"
            and required <= got
            and (forbidden or set()).isdisjoint(got)
            and data["counts"].get("verified", -1) == 0
            and data["counts"].get("certified", -1) == 0
        )
        check(name, denied, f"exit={code} status={row['status']} errors={sorted(got)}")

    def expect_pass(name: str, target: str, level: str) -> None:
        code, data, _raw = load(name)
        if data is None:
            check(name, False, f"exit={code} not json")
            return
        row = cell(data, target)
        good = (
            code == 0
            and data["ok"] is True
            and data["promoted"] is False
            and data["matrix_rewritten"] is False
            and not errors(data)
            and row["status"] == "passed"
            and row["matrix_level"] == level
            and row["recommended_level"] == level
            and row["downgrade_recommended"] is False
            and data["counts"].get("verified", -1) == 0
        )
        check(name, good, f"exit={code} status={row['status']} level={row['matrix_level']}")

    code, data, raw = load("map")
    check("map", code == 0 and "oo-source-conformance/case-map/v1" in raw and all(f"CC-{n:02d}" in raw for n in range(1, 14)), f"exit={code}")
    for name in ("usage-bare", "usage-check"):
        code, _data, _raw = load(name)
        check(name, code == 2, f"exit={code}")

    code, data, _raw = load("schema-matrix")
    check("schema-matrix", code == 1 and data is not None and errors(data) == ["matrix.schema"], f"exit={code} errors={None if data is None else errors(data)}")
    code, data, _raw = load("schema-package")
    check("schema-package", code == 1 and data is not None and errors(data) == ["package.schema"], f"exit={code} errors={None if data is None else errors(data)}")

    code, data, _raw = load("hash-mismatch")
    check(
        "hash-mismatch",
        code == 1 and data is not None and errors(data) == ["evidence.hash_mismatch"] and data["promoted"] is False,
        f"exit={code} errors={None if data is None else errors(data)}",
    )
    code, data, _raw = load("hash-case")
    if data is None:
        check("hash-case", False, f"exit={code}")
    else:
        got = set(errors(data))
        row = cell(data, "rev-w37-raw")
        check(
            "hash-case",
            code == 1 and data["ok"] is False and row["status"] == "failed" and {"evidence.hash_mismatch", "claim.missing_test"} <= got and data["promoted"] is False,
            f"exit={code} status={row['status']} errors={sorted(got)}",
        )
    code, data, _raw = load("target-mismatch")
    check(
        "target-mismatch",
        code == 1 and data is not None and errors(data) == ["evidence.target_mismatch"] and data["promoted"] is False,
        f"exit={code} errors={None if data is None else errors(data)}",
    )

    expect_pass("positive", "rev-w37-keel", "experimental")
    code, data, _raw = load("designed-bare")
    if data is None:
        check("designed-bare", False, f"exit={code}")
    else:
        statuses = {item["status"] for item in data["cells"]}
        check("designed-bare", code == 0 and data["ok"] is True and statuses == {"not_required"} and data["promoted"] is False, f"exit={code} statuses={statuses}")
    code, data, _raw = load("streak14")
    if data is None:
        check("streak14", False, f"exit={code}")
    else:
        row = cell(data, "rev-w37-streak")
        info = [item["code"] for item in data["findings"] if item["severity"] == "info"]
        check(
            "streak14",
            code == 0 and row["status"] == "not_required" and row["matrix_level"] == "designed" and row["recommended_level"] == "designed" and "nightly.not_promoted" in info and data["promoted"] is False,
            f"exit={code} info={info} recommended={row['recommended_level']}",
        )
    code, data, _raw = load("missed-day")
    if data is None:
        check("missed-day", False, f"exit={code}")
    else:
        row = nightly(data, "rev-w37-gap")
        check(
            "missed-day",
            code == 0 and row["missed_days"] == ["2026-06-02"] and row["trailing_consecutive_success_days"] == 1 and row["matrix_level"] == "designed" and data["promoted"] is False,
            f"exit={code} missed={row['missed_days']} trailing={row['trailing_consecutive_success_days']}",
        )
    code, data, _raw = load("downgrade")
    if data is None:
        check("downgrade", False, f"exit={code}")
    else:
        row = cell(data, "rev-w37-yard")
        got = set(errors(data))
        check(
            "downgrade",
            code == 1 and {"nightly.streak_short", "nightly.failure_downgrade"} <= got and row["recommended_level"] == "experimental" and row["downgrade_recommended"] is True and data["matrix_rewritten"] is False,
            f"exit={code} recommended={row['recommended_level']} errors={sorted(got)}",
        )
    code, data, _raw = load("manual-only")
    if data is None:
        check("manual-only", False, f"exit={code}")
    else:
        got = set(errors(data))
        row = nightly(data, "rev-w37-manual")
        check(
            "manual-only",
            code == 1 and {"claim.manual_assertion", "claim.missing_test"} <= got and row["manual_runs"] == 1 and row["suite_runs"] == 0,
            f"exit={code} errors={sorted(got)} manual={row['manual_runs']} suite={row['suite_runs']}",
        )
    code, data, _raw = load("cloud-container")
    check("cloud-container", code == 1 and data is not None and errors(data) == ["cloud.container_scope"] and data["promoted"] is False, f"exit={code} errors={None if data is None else errors(data)}")
    code, data, _raw = load("silent-fallback")
    check("silent-fallback", code == 1 and data is not None and errors(data) == ["claim.silent_fallback"] and data["promoted"] is False, f"exit={code} errors={None if data is None else errors(data)}")

    expect_deny("drift", "rev-w37-drift", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    code, data, _raw = load("drift")
    if data is not None:
        detail = " ".join(item["detail"] for item in data["findings"])
        check("drift-detail", "15.2" in detail and "16.9" in detail and "canonical 15.2" in detail, detail)
    code, data, _raw = load("drift-nightly")
    if data is None:
        check("drift-nightly", False, f"exit={code}")
    else:
        row = nightly(data, "rev-w37-drift")
        got = set(errors(data))
        check(
            "drift-nightly",
            code == 1 and row["trailing_consecutive_success_days"] == 0 and row["suite_runs"] == 0 and "nightly.failure_downgrade" not in got and cell(data, "rev-w37-drift")["status"] == "failed",
            f"exit={code} trailing={row['trailing_consecutive_success_days']} suite={row['suite_runs']} errors={sorted(got)}",
        )

    expect_deny("empty", "rev-w37-empty", {"version.identity_malformed", "claim.manual_assertion", "claim.missing_test"})
    expect_deny(
        "missing",
        "rev-w37-missing",
        {"evidence.malformed", "claim.manual_assertion", "claim.missing_test"},
        {"version.identity_missing"},
    )
    expect_deny(
        "missing-valid",
        "rev-w37-missing-valid",
        {"version.identity_missing", "claim.manual_assertion", "claim.missing_test"},
        {"evidence.malformed", "version.identity_duplicate"},
    )
    expect_deny("malformed", "rev-w37-malformed", {"version.identity_malformed", "claim.manual_assertion", "claim.missing_test"})
    expect_deny("unknown", "rev-w37-unknown", {"version.identity_unknown", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("patch", "rev-w37-patch", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("major", "rev-w37-major", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("prefix", "rev-w37-prefix", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_pass("suffix-same", "rev-w37-suffix-same", "experimental")
    expect_deny("suffix-mismatch", "rev-w37-suffix-mis", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("suffix-confusion", "rev-w37-suffix-mix", {"version.identity_unknown", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_pass("mysql-suffix", "rev-w37-my-suffix", "experimental")
    expect_deny("mysql-prefix", "rev-w37-my-prefix", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("compat", "rev-w37-compat", {"version.identity_unknown", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("whitespace", "rev-w37-space", {"version.identity_unknown", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("oracle-alias", "rev-w37-oracle", {"version.identity_mismatch", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_deny("inherited-pg", "rev-w37-inh-pg", {"version.identity_unknown", "claim.missing_test"}, {"claim.manual_assertion"})
    expect_pass("inherited-mysql", "rev-w37-inh-my", "experimental")
    expect_deny("driver-bad", "rev-w37-driver-bad", {"package.driver_digest", "claim.manual_assertion", "claim.missing_test"})
    expect_pass("driver-unbound", "rev-w37-driver", "experimental")

    def expect_duplicate(name: str, target: str, key: str) -> None:
        code, data, _raw = load(name)
        if data is None:
            check(name, False, f"exit={code} not json")
            return
        got = set(errors(data))
        row = cell(data, target)
        details = [item["detail"] for item in data["findings"] if item["code"] == "version.identity_duplicate"]
        denied = (
            code == 1
            and data["ok"] is False
            and data["promoted"] is False
            and data["matrix_rewritten"] is False
            and row["status"] == "failed"
            and {"version.identity_duplicate", "claim.missing_test"} <= got
            and "claim.manual_assertion" not in got
            and "version.identity_mismatch" not in got
            and details == [key]
            and data["counts"].get("verified", -1) == 0
            and data["counts"].get("certified", -1) == 0
        )
        check(name, denied, f"exit={code} status={row['status']} errors={sorted(got)} detail={details}")

    def expect_no_credit(name: str, target: str, required: set[str], forbidden: set[str] | None = None) -> None:
        code, data, _raw = load(name)
        if data is None:
            check(name, False, f"exit={code} not json")
            return
        got = set(errors(data))
        row = cell(data, target)
        night = nightly(data, target)
        withheld = (
            code == 1
            and data["ok"] is False
            and data["promoted"] is False
            and data["matrix_rewritten"] is False
            and row["status"] == "failed"
            and required <= got
            and (forbidden or set()).isdisjoint(got)
            and night["trailing_consecutive_success_days"] == 0
            and night["suite_runs"] == 0
            and "nightly.failure_downgrade" not in got
        )
        check(
            name,
            withheld,
            f"exit={code} status={row['status']} trailing={night['trailing_consecutive_success_days']} suite={night['suite_runs']} errors={sorted(got)}",
        )

    for name, target, key in (
        ("dup-service-last-match", "rev-w37-dup-match", "service_version"),
        ("dup-service-last-drift", "rev-w37-dup-drift", "service_version"),
        ("dup-exact-last-match", "rev-w37-dup-exact", "exact_version"),
        ("dup-exact-last-drift", "rev-w37-dup-exact-drift", "exact_version"),
        ("dup-service-same", "rev-w37-dup-same", "service_version"),
        ("dup-tested-drift", "rev-w37-dup-tested", "tested_versions"),
        ("dup-escaped", "rev-w37-dup-esc", "service_version"),
        ("dup-escaped-v", "rev-w37-dup-escv", "service_version"),
        ("dup-nested-service", "rev-w37-dup-nest", "service_version"),
        ("dup-nested-identity", "rev-w37-dup-ident", "identity"),
    ):
        expect_duplicate(name, target, key)
    code, data, _raw = load("dup-buried")
    if data is None:
        check("dup-buried", False, f"exit={code} not json")
    else:
        got = set(errors(data))
        row = cell(data, "rev-w37-dup-buried")
        check(
            "dup-buried",
            code == 1 and data["ok"] is False and row["status"] == "failed" and "version.identity_duplicate" in got and data["promoted"] is False,
            f"exit={code} status={row['status']} errors={sorted(got)}",
        )
    expect_no_credit(
        "dup-service-nightly",
        "rev-w37-dup-match",
        {"version.identity_duplicate", "claim.missing_test"},
        {"claim.manual_assertion", "nightly.failure_downgrade"},
    )
    expect_no_credit(
        "missing-valid-nightly",
        "rev-w37-missing-valid",
        {"version.identity_missing", "claim.manual_assertion", "claim.missing_test"},
        {"nightly.failure_downgrade"},
    )
    expect_no_credit(
        "malformed-nightly",
        "rev-w37-malformed",
        {"version.identity_malformed", "claim.manual_assertion", "claim.missing_test"},
        {"nightly.failure_downgrade"},
    )
    expect_no_credit(
        "missing-nightly",
        "rev-w37-missing",
        {"evidence.malformed", "claim.manual_assertion", "claim.missing_test"},
        {"version.identity_missing", "nightly.failure_downgrade"},
    )

    code, data, _raw = load("current-matrix")
    if data is None:
        check("current-matrix", False, f"exit={code}")
    else:
        hist = data["historical_pg"]
        consumer = data["source_support_consumer"]
        check(
            "current-matrix",
            code == 0
            and data["ok"] is True
            and data["promoted"] is False
            and data["matrix_rewritten"] is False
            and data["counts"] == {"candidate": 38, "certified": 0, "designed": 181, "excluded": 8, "experimental": 0, "verified": 0}
            and hist["classification"] == "historical_draft_preview_not_conformance_suite"
            and hist["satisfies_cc_suite"] is False
            and hist["promotes_grade"] is False
            and consumer["status"] == "pending"
            and consumer["mutable_workspace"] == "not-read"
            and "parse_support_matrix-absent" in consumer["protocol_commit"]
            and "29597079f7feacc6e3934b5c803686fc80567a93" in consumer["protocol_commit"],
            f"exit={code} counts={data['counts']}",
        )
    code, data, _raw = load("matrix-copy")
    before = (EV / "matrix-copy-before.sha256").read_text().split()[0]
    after = (EV / "matrix-copy-after.sha256").read_text().split()[0]
    cmp_exit = int((EV / "matrix-copy-cmp.exit").read_text().strip())
    check("matrix-copy", code == 0 and before == after and cmp_exit == 0 and data is not None and data["promoted"] is False, f"exit={code} cmp={cmp_exit}")

    promoted = []
    for path in sorted(PROBES.glob("*.stdout")):
        text = path.read_text().strip()
        if not text.startswith("{"):
            continue
        parsed = json.loads(text)
        if parsed.get("promoted") or parsed.get("matrix_rewritten"):
            promoted.append(path.name)
    check("no-promotion", not promoted, ",".join(promoted) or "all reports false")

    result = {"failures": failures, "notes": notes, "failure_count": len(failures)}
    (EV / "judgment.json").write_text(json.dumps(result, indent=2) + "\n")
    print(f"failures={len(failures)}")
    for line in notes:
        print(line)
    raise SystemExit(1 if failures else 0)


if __name__ == "__main__":
    main()
