#!/usr/bin/env python3
"""Write synthetic evidence fixtures. Hashes are SHA-256 of the raw bytes."""

import hashlib
import json
from datetime import date, timedelta
from pathlib import Path

ROOT = Path(__file__).resolve().parent
DRIVER = hashlib.sha256(b"fixture-driver-not-a-secret").hexdigest()
CASES = ["CC-01", "CC-02", "CC-03", "CC-04", "CC-11"]
REVIEWERS = [
    {"role": "data", "name": "Ada Data"},
    {"role": "dba", "name": "Bea Dba"},
    {"role": "security", "name": "Cy Security"},
]


def sha(body: bytes) -> str:
    return hashlib.sha256(body).hexdigest()


def flip(digest: str) -> str:
    last = int(digest[-1], 16) ^ 1
    return digest[:-1] + format(last, "x")


def days(start: str, count: int):
    cursor = date.fromisoformat(start)
    out = []
    for _ in range(count):
        out.append(cursor.isoformat())
        cursor += timedelta(days=1)
    return out


def matrix(level: str, versions, variant: str = "self-managed", target: str = "fixture-pg", extra_caps=None):
    caps = {"batch_full": level, "cdc": "designed"}
    if extra_caps:
        caps = extra_caps
    lines = [
        "schema: oo-support-matrix/v1",
        "captured: 2026-10-09",
        "levels: [candidate, designed, experimental, verified, certified, excluded]",
        'note: "synthetic fixture; not the admission matrix"',
        "targets:",
        f"  - id: {target}",
        "    family: postgres",
        "    product: PostgreSQL",
    ]
    if versions is not None:
        rendered = ", ".join(f'"{item}"' for item in versions)
        lines.append(f"    versions: [{rendered}]")
    lines.append(f"    variant: {variant}")
    lines.append("    caps:")
    for key, value in caps.items():
        lines.append(f"      {key}: {value}")
    lines.append("")
    return "\n".join(lines)


def package(target, exact, variant, evidence_type, cycle, bodies, rejections, reviewers, deployment="public"):
    cases = []
    for case_id, body in bodies:
        cases.append({
            "id": case_id,
            "capability": "batch_full",
            "disposition": "pass",
            "raw_exit": 0,
            "raw_sha256": sha(body),
            "raw_path": f"raw/{case_id}.txt",
        })
    return {
        "schema": "oo-source-conformance/evidence-package/v1",
        "evidence_id": "fixture-evidence-0001",
        "target_id": target,
        "exact_version": exact,
        "tested_versions": [exact],
        "variant": variant,
        "driver_digest": DRIVER,
        "suite_version": "cc-map/1",
        "evidence_type": evidence_type,
        "deployment_scope": deployment,
        "identity": {"topology": "fixture-single-node", "service_version": exact},
        "cloud": {"applies": evidence_type != "container", "budget_exceeded": False, "cycle_status": cycle},
        "cases": cases,
        "rejections": rejections,
        "reviewers": reviewers,
    }


def rejection(body: bytes, silent: bool, name: str):
    return {
        "body_name": name,
        "body": body,
        "row": {
            "capability": "cdc",
            "code": "CAPABILITY_UNVERIFIED",
            "silent_fallback": silent,
            "raw_exit": 1,
            "raw_sha256": sha(body),
            "raw_path": f"raw/{name}",
        },
    }


def nightly(runs):
    return {
        "schema": "oo-source-conformance/nightly-registry/v1",
        "entries": [{
            "target_id": "fixture-pg",
            "capability": "batch_full",
            "runs": runs,
        }],
    }


def run(day: str, exit_code: int, origin: str = "suite"):
    token = day.replace("-", "")
    return {
        "calendar_day": day,
        "raw_exit": exit_code,
        "evidence_id": f"fix-{token}",
        "origin": origin,
    }


def write_tree(name, yaml, manifest, files, registry):
    directory = ROOT / name
    if directory.exists():
        for path in sorted(directory.rglob("*"), reverse=True):
            if path.is_file():
                path.unlink()
    package_dir = directory / "package"
    (package_dir / "raw").mkdir(parents=True, exist_ok=True)
    (directory / "matrix.yaml").write_text(yaml)
    (package_dir / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    for filename, body in files.items():
        (package_dir / "raw" / filename).write_bytes(body)
    if registry is not None:
        (directory / "nightly.json").write_text(json.dumps(registry, indent=2) + "\n")


def full_bodies(omit=()):
    bodies = []
    files = {}
    for case_id in CASES:
        if case_id in omit:
            continue
        body = f"{case_id} batch_full pass\n".encode()
        bodies.append((case_id, body))
        files[f"{case_id}.txt"] = body
    return bodies, files


def fourteen():
    return [run(day, 0) for day in days("2026-09-01", 14)]


def standard(name, level, versions, variant="self-managed", omit=(), nightly_runs="fourteen",
             evidence_type="container", cycle="not_cloud", target="fixture-pg",
             package_target=None, extra_caps=None, silent_extra=False, reviewers=True,
             deployment="public"):
    bodies, files = full_bodies(omit)
    good = rejection(b"CAPABILITY_UNVERIFIED cdc\n", False, "reject-cdc.txt")
    rows = [good["row"]]
    files[good["body_name"]] = good["body"]
    if silent_extra:
        bad = rejection(b"silent fallback cdc\n", True, "reject-cdc-silent.txt")
        rows.append(bad["row"])
        files[bad["body_name"]] = bad["body"]
    manifest = package(
        package_target or target,
        "16.4",
        variant,
        evidence_type,
        cycle,
        bodies,
        rows,
        REVIEWERS if reviewers else [],
        deployment,
    )
    if nightly_runs == "fourteen":
        registry = nightly(fourteen())
    elif nightly_runs == "thirteen":
        registry = nightly([run(day, 0) for day in days("2026-09-01", 13)])
    elif nightly_runs == "two-fail":
        registry = nightly([run("2026-10-01", 1), run("2026-10-02", 1)])
    else:
        registry = None
    write_tree(name, matrix(level, versions, variant, target, extra_caps), manifest, files, registry)


def tamper():
    standard("tamper", "verified", ["16.4"])
    manifest_path = ROOT / "tamper" / "package" / "manifest.json"
    manifest = json.loads(manifest_path.read_text())
    for case in manifest["cases"]:
        if case["id"] == "CC-01":
            case["raw_sha256"] = flip(case["raw_sha256"])
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")


def excluded():
    body = b"CC-01 cdc pass\n"
    manifest = {
        "schema": "oo-source-conformance/evidence-package/v1",
        "evidence_id": "fixture-evidence-0001",
        "target_id": "fixture-pg",
        "exact_version": "16.4",
        "tested_versions": ["16.4"],
        "variant": "self-managed",
        "driver_digest": DRIVER,
        "suite_version": "cc-map/1",
        "evidence_type": "container",
        "deployment_scope": "public",
        "identity": {"topology": "fixture-single-node", "service_version": "16.4"},
        "cloud": {"applies": False, "budget_exceeded": False, "cycle_status": "not_cloud"},
        "cases": [{
            "id": "CC-01",
            "capability": "cdc",
            "disposition": "pass",
            "raw_exit": 0,
            "raw_sha256": sha(body),
            "raw_path": "raw/CC-01.txt",
        }],
        "rejections": [],
        "reviewers": [],
    }
    write_tree(
        "excluded-pass",
        matrix("excluded", ["16.4"], extra_caps={"cdc": "excluded"}),
        manifest,
        {"CC-01.txt": body},
        None,
    )


def real_nightly():
    def entry(target, capability, runs):
        return {"target_id": target, "capability": capability, "runs": runs}

    catalog = [run("2026-08-01", 0, "manual")]
    catalog.extend(run(day, 0) for day in days("2026-09-01", 14))
    # manual evidence id must still be valid; run() uses the day.
    catalog[0]["evidence_id"] = "manual-pg-cat-0801"
    batch = [
        run("2026-10-01", 0),
        run("2026-10-02", 0),
        run("2026-10-03", 0),
        run("2026-10-05", 0),
    ]
    mysql = [run("2026-10-01", 1), run("2026-10-02", 1)]
    document = {
        "schema": "oo-source-conformance/nightly-registry/v1",
        "entries": [
            entry("postgres-selfmanaged", "batch_full", batch),
            entry("postgres-selfmanaged", "catalog", catalog),
            entry("mysql8-selfmanaged", "cdc", mysql),
        ],
    }
    (ROOT / "real-nightly.json").write_text(json.dumps(document, indent=2) + "\n")


def manual_matrix():
    directory = ROOT / "manual-verified"
    directory.mkdir(parents=True, exist_ok=True)
    (directory / "matrix.yaml").write_text(matrix("verified", ["16.4"]))


def main():
    standard("ok-verified", "verified", ["16.4"])
    tamper()
    standard("missing-test", "verified", ["16.4"], omit=("CC-04",))
    standard("mismatched-target", "designed", ["16.4"], target="fixture-designed",
             package_target="missing-target", nightly_runs="none", reviewers=False,
             extra_caps={"batch_full": "designed"})
    standard("silent-fallback", "verified", ["16.4"], silent_extra=True)
    standard("cloud-container", "experimental", ["16.4"], variant="rds", nightly_runs="none", reviewers=False)
    standard("version-range", "verified", ["13-17"])
    standard("verified-partial", "verified", ["16.4", "17.6"])
    standard("budget-cycle", "verified", ["16.4"], cycle="not_verified_this_cycle")
    standard("downgrade", "verified", ["16.4"], nightly_runs="two-fail")
    standard("streak-13", "verified", ["16.4"], nightly_runs="thirteen")
    standard("certified-incomplete", "certified", ["16.4"])
    standard("experimental-range", "experimental", ["13-17"], nightly_runs="none", reviewers=False)
    standard("experimental-empty-version", "experimental", None, nightly_runs="none", reviewers=False)
    excluded()
    manual_matrix()
    real_nightly()
    (ROOT / "driver-digest.txt").write_text(DRIVER + "\n")


if __name__ == "__main__":
    main()
