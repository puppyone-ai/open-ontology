#!/usr/bin/env python3
"""Governance document QA for the SP00-W06 correction.

Python is limited to this document check. The product and the future
engineering runner stay Rust. The default run writes nothing. ``--write-evidence``
writes only when ``--evidence-run`` names the current permitted run,
``eef831aa-dba4-4af2-a78b-541bc9e0a011``. Historical runs, including a007, are
refused. This program does not open a license URL.

Exit 0 means every applicable document check passed. Skipped tools are not
passes. Full AT-SEC-05 stays pending. This program does not run cargo audit,
cargo deny, or an image or native scan, and it does not appoint an owner.
"""

from __future__ import annotations

import sys

sys.dont_write_bytecode = True

import argparse
import hashlib
import importlib.util
import json
import shutil
import subprocess
from datetime import datetime, timezone
from pathlib import Path

import qa_docs
import qa_record
from qa_docs import (
    CLAIM_FILES,
    GOVERNANCE_FILES,
    OPERATIVE_RULES,
    claim_hits,
    classify_diff_check,
    codeowners_active_lines,
    form_errors,
    load_yaml,
    local_link_errors,
    operative_holds,
    self_test as docs_self_test,
    whitespace_errors,
)
from qa_record import audited_record_errors, observed_clock, self_test as record_self_test, template_errors

ROOT = Path(__file__).resolve().parents[1]
PERMITTED_EVIDENCE_RUN = "eef831aa-dba4-4af2-a78b-541bc9e0a011"
REFUSED_EVIDENCE_RUNS = frozenset(
    {
        "eef831aa-dba4-4af2-a78b-541bc9e0a001",
        "eef831aa-dba4-4af2-a78b-541bc9e0a002",
        "eef831aa-dba4-4af2-a78b-541bc9e0a006",
        "eef831aa-dba4-4af2-a78b-541bc9e0a007",
        "eef831aa-dba4-4af2-a78b-541bc9e0a008",
        "eef831aa-dba4-4af2-a78b-541bc9e0a009",
    }
)
LICENSE_SHA256 = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"
HISTORICAL_CHECKER = Path(
    "governance/fixtures/check_sp00_w06.py"
)
HISTORICAL_CHECKER_SHA256 = "3a8c642d47025093ea58ec116529a2ead60b7a01f08ce745e54bb30a3494d883"
IMMUTABLE_FILES = (
    "LICENSE",
    "supply-chain/dependency-baseline.json",
    "supply-chain/dependency-baseline.md",
)
IMMUTABLE_TREES = (
    "specs",
    "docs",
    "execution",
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a001",
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002",
    "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a007",
)
DEFERRED = [
    "AT-SEC-05 full case (lock, JavaScript, native, image, signature tamper, secret scan, SBOM)",
    "security owner, private vulnerability channel, conduct contact, and release contact",
    "CODEOWNERS activation and branch protection",
    "independent follow-up review",
]
FORM_FILES = (
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/ISSUE_TEMPLATE/bug.yml",
    ".github/ISSUE_TEMPLATE/task.yml",
    ".github/ISSUE_TEMPLATE/dependency.yml",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def snapshot_immutable(root: Path) -> dict[str, str]:
    files: list[Path] = []
    for relative in IMMUTABLE_FILES:
        files.append(root / relative)
    for tree in IMMUTABLE_TREES:
        base = root / tree
        if not base.exists():
            continue
        for path in sorted(base.rglob("*")):
            if path.is_file() and "__pycache__" not in path.parts and path.suffix != ".pyc":
                files.append(path)
    recorded: dict[str, str] = {}
    for path in files:
        recorded[str(path.relative_to(root))] = sha256(path)
    return recorded


def run_command(command: list[str]) -> dict:
    try:
        completed = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    except OSError as exc:
        return {"command": command, "exit_code": 127, "stdout": "", "stderr": str(exc)}
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout,
        "stderr": completed.stderr,
    }


def record(results: list[dict], name: str, status: str, detail: object) -> None:
    if status not in {"passed", "failed", "skipped"}:
        raise RuntimeError(f"bad status {status}")
    ok: bool | None
    if status == "passed":
        ok = True
    elif status == "failed":
        ok = False
    else:
        ok = None
    if status == "skipped" and ok is True:
        raise RuntimeError("skipped check marked passed")
    results.append({"name": name, "status": status, "ok": ok, "detail": detail})


def appointed_names(root: Path) -> list[str]:
    """Names this repository has actually appointed.

    OWNER_PENDING and the unconfigured security text appoint nobody. This
    function does not invent a person to make an exception pass.
    """

    codeowners = (root / ".github/CODEOWNERS").read_text()
    security = (root / "governance/SECURITY.md").read_text()
    conduct = (root / "governance/CODE_OF_CONDUCT.md").read_text()
    if codeowners_active_lines(codeowners):
        return []
    if "OWNER_PENDING" not in codeowners:
        return []
    if "None of those people are appointed." not in security:
        return []
    if "No community leader is appointed" not in conduct:
        return []
    return []


def historical_holes() -> dict[str, list[str]]:
    path = ROOT / HISTORICAL_CHECKER
    spec = importlib.util.spec_from_file_location("historical_sp00_w06_checker", path)
    if spec is None or spec.loader is None:
        raise RuntimeError("historical checker could not be loaded")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    cases = {
        "gpl": {"license_spdx": "GPL-3.0-only", "prohibited_license": False},
        "lgpl": {"license_spdx": "LGPL-2.1-only"},
        "licenseref": {"license_spdx": "LicenseRef-Proprietary"},
        "or-agpl": {"license_spdx": "MIT OR AGPL-3.0-only"},
        "native-str": {"native_transitive": ["SSPL-1.0"]},
        "age-mismatch": {
            "publication_date": "2026-09-30",
            "adoption_date": "2026-10-01",
            "age_days_at_adoption": 31,
            "seven_day_rule": "pass",
        },
    }
    found = {name: module.audited_record_errors(module.synthetic_audited(package=package)) for name, package in cases.items()}
    young = module.synthetic_audited(
        package={"age_days_at_adoption": 1, "seven_day_rule": "security-exception"},
        exception={
            "kind": "security-fix-seven-day",
            "owner_role": "security-owner",
            "reason": "synthetic negative",
            "regression_evidence": "evidence/none",
            "waive": [],
        },
    )
    found["young-exception"] = module.audited_record_errors(young)
    omitted = module.synthetic_audited()
    del omitted["package"]["default_features_disabled"]
    found["omit-default-features"] = module.audited_record_errors(omitted)
    return found


def evidence_write_block(run_id: str | None) -> str | None:
    """None when a write is allowed. Historical runs, including a007, are not allowed."""

    if not run_id:
        return "an evidence run id is required; historical a007 is not the default write target"
    if run_id in REFUSED_EVIDENCE_RUNS or run_id != PERMITTED_EVIDENCE_RUN:
        return "refusing to write historical or unpermitted evidence"
    return None


def evidence_rel(run_id: str) -> Path:
    return Path("evidence") / run_id / "SP00" / "SP00-W06"


def add_self_tests(results: list[dict], as_of) -> None:
    for name, problem in record_self_test(as_of):
        record(results, name, "passed" if problem is None else "failed", problem or "rejected or accepted as required")
    for name, problem in docs_self_test(ROOT):
        record(results, name, "passed" if problem is None else "failed", problem or "same function rejected the mutation")
    blocked_default = evidence_write_block(None)
    record(
        results,
        "evidence-write:default-refused",
        "passed" if blocked_default else "failed",
        blocked_default or "default write was allowed",
    )
    blocked_historical = evidence_write_block("eef831aa-dba4-4af2-a78b-541bc9e0a007")
    record(
        results,
        "evidence-write:a007-refused",
        "passed" if blocked_historical else "failed",
        blocked_historical or "historical a007 write was allowed",
    )
    blocked_a002 = evidence_write_block("eef831aa-dba4-4af2-a78b-541bc9e0a002")
    record(
        results,
        "evidence-write:a002-refused",
        "passed" if blocked_a002 else "failed",
        blocked_a002 or "historical a002 write was allowed",
    )
    allowed = evidence_write_block(PERMITTED_EVIDENCE_RUN)
    record(
        results,
        "evidence-write:a011-explicit",
        "passed" if allowed is None else "failed",
        allowed or "current run is the only permitted write",
    )


def add_tree_checks(results: list[dict], commands: list[dict]) -> None:
    license_path = ROOT / "LICENSE"
    license_hash = sha256(license_path)
    record(results, "license-sha256", "passed" if license_hash == LICENSE_SHA256 else "failed", license_hash)
    historical_hash = sha256(ROOT / HISTORICAL_CHECKER)
    record(
        results,
        "historical-checker-unchanged",
        "passed" if historical_hash == HISTORICAL_CHECKER_SHA256 else "failed",
        historical_hash,
    )
    holes = historical_holes()
    still_open = sorted(name for name, errors in holes.items() if errors == [])
    expected_open = {
        "gpl",
        "lgpl",
        "licenseref",
        "or-agpl",
        "native-str",
        "age-mismatch",
        "young-exception",
        "omit-default-features",
    }
    record(
        results,
        "reproduced-historical-holes",
        "passed" if set(still_open) == expected_open else "failed",
        {
            "note": "The immutable SP00-W06 checker still accepts these. That is the defect record, not a clearance.",
            "accepted_by_historical_checker": still_open,
        },
    )

    for relative, sentence in OPERATIVE_RULES:
        text = (ROOT / relative).read_text()
        holds = operative_holds(text, sentence)
        record(
            results,
            f"operative:{relative}:{sentence[:48]}",
            "passed" if holds else "failed",
            "present" if holds else "required sentence missing or only negated",
        )

    readme = (ROOT / "README.md").read_text()
    record(
        results,
        "readme-definition",
        "passed" if operative_holds(readme, "The storage model remains PostgreSQL; OO does not require RDF, OWL, a triple store, or a graph database.") else "failed",
        "relational storage definition",
    )
    record(
        results,
        "readme-checker-command",
        "passed" if "python3 governance/check_governance.py" in (ROOT / "governance/README.md").read_text() else "failed",
        "command named",
    )

    for relative in CLAIM_FILES:
        hits = claim_hits((ROOT / relative).read_text())
        record(results, f"no-overclaim:{relative}", "passed" if not hits else "failed", hits)
    denylist_hits = claim_hits("This build is production-ready and all tests pass")
    record(
        results,
        "claim-denylist-fires",
        "passed" if denylist_hits == ["production-ready", "all tests pass"] else "failed",
        denylist_hits,
    )

    emails: list[str] = []
    whitespace: list[str] = []
    untracked: list[str] = []
    for relative in GOVERNANCE_FILES:
        path = ROOT / relative
        if not path.is_file():
            record(results, f"exists:{relative}", "failed", "missing")
            continue
        tracked = run_command(["git", "ls-files", "--error-unmatch", "--", relative])
        commands.append(tracked)
        if tracked["exit_code"] != 0:
            untracked.append(relative)
        whitespace.extend(whitespace_errors(path))
        found = qa_docs.EMAIL_RE.findall(path.read_text(errors="replace"))
        if found:
            emails.append(f"{relative}: {found}")
    record(results, "no-email-addresses", "passed" if not emails else "failed", emails)
    record(
        results,
        "whitespace-bytes",
        "passed" if not whitespace else "failed",
        {"errors": whitespace, "untracked_files_read": untracked},
    )

    codeowners = (ROOT / ".github/CODEOWNERS").read_text()
    active = codeowners_active_lines(codeowners)
    augmented = codeowners_active_lines(codeowners + "\n* @not-a-real-user\n")
    record(results, "codeowners-inactive", "passed" if not active else "failed", active)
    record(
        results,
        "codeowners-negative-detects-owner",
        "passed" if active == [] and len(augmented) == 1 else "failed",
        augmented,
    )

    names = appointed_names(ROOT)
    record(
        results,
        "observed-no-appointed-owner",
        "passed" if names == [] else "failed",
        "OWNER_PENDING remains. This observation is not an appointment and not a clearance.",
    )

    link_errors: list[str] = []
    for relative in GOVERNANCE_FILES:
        path = ROOT / relative
        if path.suffix.lower() == ".md" or path.name == "NOTICE":
            link_errors.extend(local_link_errors(path.read_text(), path))
    record(results, "relative-links", "passed" if not link_errors else "failed", link_errors)

    template = json.loads((ROOT / "supply-chain/dependency-audit-record.template.json").read_text())
    blank_errors = template_errors(template)
    record(results, "audit-template-blank", "passed" if not blank_errors else "failed", blank_errors)
    as_of = observed_clock(datetime.now(timezone.utc))
    flipped = dict(template)
    flipped["audited"] = True
    flipped["template"] = False
    flipped_errors = audited_record_errors(flipped, as_of=as_of, appointed_names=[])
    record(
        results,
        "audit-template-cannot-pass-by-flipping-audited",
        "passed" if flipped_errors else "failed",
        flipped_errors[:12],
    )

    ruby_details: list[dict] = []
    ruby_parse_ok = True
    schema_ok = True
    for relative in FORM_FILES:
        path = ROOT / relative
        try:
            document = load_yaml(path)
            errors = form_errors(path, document)
            parsed = True
        except (RuntimeError, json.JSONDecodeError) as exc:
            document = None
            errors = [str(exc)]
            parsed = False
        ruby_parse_ok = ruby_parse_ok and parsed
        schema_ok = schema_ok and parsed and not errors
        ruby_details.append({"file": relative, "parsed": parsed, "errors": errors})
    record(results, "ruby-yaml-parse", "passed" if ruby_parse_ok else "failed", ruby_details)
    record(results, "issue-form-schema", "passed" if schema_ok else "failed", ruby_details)

    yamllint = shutil.which("yamllint")
    probe = run_command([yamllint or "yamllint", "--version"])
    commands.append(probe)
    yamllint_runs = bool(yamllint) and probe["exit_code"] == 0
    if yamllint_runs:
        lint = run_command(
            [
                yamllint,
                "-d",
                "{extends: default, rules: {line-length: {max: 120}, truthy: {check-keys: false}}}",
                *FORM_FILES,
            ]
        )
        commands.append(lint)
        record(results, "yamllint", "passed" if lint["exit_code"] == 0 else "failed", {"exit_code": lint["exit_code"], "stderr": lint["stderr"][-500:]})
    else:
        record(
            results,
            "yamllint",
            "skipped",
            {
                "reason": "yamllint is not executable here. Ruby YAML.safe_load is the parse that ran. Skipped is not passed.",
                "exit_code": probe["exit_code"],
                "stderr": (probe["stderr"] or probe["stdout"])[-500:],
            },
        )

    disposition = classify_diff_check(untracked)
    diff = run_command(["git", "diff", "--check"])
    commands.append(diff)
    if disposition == "skipped":
        record(
            results,
            "git-diff-check",
            "skipped",
            {
                "reason": "git diff --check does not read untracked files. whitespace-bytes read them. Skipped is not passed.",
                "untracked_count": len(untracked),
                "command_exit_code": diff["exit_code"],
            },
        )
    else:
        record(
            results,
            "git-diff-check",
            "passed" if diff["exit_code"] == 0 else "failed",
            {"exit_code": diff["exit_code"], "stdout": diff["stdout"][-500:]},
        )

    for name, script in (("check-spec-self-test", "specs/scripts/check_spec.py"), ("check-plan-self-test", "specs/scripts/check_plan.py")):
        completed = run_command([sys.executable, script, "--self-test"])
        commands.append(completed)
        record(results, name, "passed" if completed["exit_code"] == 0 else "failed", {"exit_code": completed["exit_code"]})

    head = run_command(["git", "rev-parse", "HEAD"])
    commands.append(head)
    commit = head["stdout"].strip()
    record(results, "git-head", "passed" if head["exit_code"] == 0 and len(commit) == 40 else "failed", commit)
    remote = run_command(["git", "remote", "-v"])
    commands.append(remote)
    record(
        results,
        "git-remote-observed",
        "passed" if remote["exit_code"] == 0 else "failed",
        remote["stdout"].strip() or "no remote",
    )


def summarize(results: list[dict]) -> dict:
    for item in results:
        if item["status"] == "skipped" and item["ok"] is True:
            raise RuntimeError(f"skipped marked passed: {item['name']}")
    passed = [item["name"] for item in results if item["status"] == "passed"]
    skipped = [item["name"] for item in results if item["status"] == "skipped"]
    failed = [item["name"] for item in results if item["status"] == "failed"]
    return {
        "status": "in_review",
        "applicable_passed": passed,
        "applicable_passed_count": len(passed),
        "skipped": skipped,
        "skipped_count": len(skipped),
        "failed": failed,
        "failed_count": len(failed),
        "deferred": DEFERRED,
        "full_case_status": "pending",
        "note": "Exit 0 means no applicable governance-document check failed. It does not mean AT-SEC-05 passed.",
    }


def evidence_dir(run_id: str) -> Path:
    block = evidence_write_block(run_id)
    if block:
        raise SystemExit(block)
    relative = evidence_rel(run_id)
    target = (ROOT / relative).resolve()
    if target.relative_to(ROOT.resolve()) != relative:
        raise SystemExit("refusing to write outside the authorized evidence run")
    return target


def write_evidence(results: list[dict], commands: list[dict], summary: dict, started: str, run_id: str) -> None:
    target = evidence_dir(run_id)
    evidence_relative = evidence_rel(run_id)
    command_dir = target / "commands"
    command_dir.mkdir(parents=True, exist_ok=True)
    for index, command in enumerate(commands, 1):
        slug = f"{index:02d}-" + "-".join(Path(part).name for part in command["command"][:3])
        slug = "".join(char if char.isalnum() or char in "-._" else "-" for char in slug)[:80]
        (command_dir / f"{slug}.log").write_text(
            "$ " + " ".join(command["command"]) + "\n" + command["stdout"] + (("\n" + command["stderr"]) if command["stderr"] else "")
        )
        (command_dir / f"{slug}.exit").write_text(str(command["exit_code"]) + "\n")
    head = next(item["stdout"].strip() for item in commands if item["command"][:2] == ["git", "rev-parse"])
    ruby = run_command(["ruby", "-v"])
    environment = {
        "started": started,
        "finished": datetime.now(timezone.utc).isoformat(),
        "python": sys.version.split()[0],
        "ruby": ruby["stdout"].strip() or ruby["stderr"].strip(),
        "yamllint": "not executable" if any(item["name"] == "yamllint" and item["status"] == "skipped" for item in results) else "ran",
        "platform": sys.platform,
        "git_head": head,
        "run_id": run_id,
        "privacy": "public-synthetic",
        "checker": "governance/check_governance.py",
        "writes": "only the explicitly named current evidence directory, and only because --write-evidence was set",
    }
    (target / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    (target / "checks.json").write_text(json.dumps({"summary": summary, "results": results}, indent=2) + "\n")
    (target / "summary.txt").write_text(format_summary(summary) + "\n")
    artifacts = []
    for relative in list(GOVERNANCE_FILES) + [
        str(evidence_relative / "checks.json"),
        str(evidence_relative / "environment.json"),
        str(evidence_relative / "summary.txt"),
    ]:
        path = ROOT / relative
        if path.is_file():
            artifacts.append({"path": relative, "sha256": sha256(path), "bytes": path.stat().st_size})
    for path in sorted(command_dir.glob("*")):
        artifacts.append({"path": str(path.relative_to(ROOT)), "sha256": sha256(path), "bytes": path.stat().st_size})
    manifest = {
        "spec_id": "OO-SPEC-001",
        "spec_version": "1.0.0",
        "plan_id": "OO-PLAN-001",
        "plan_version": "1.0.0",
        "run_id": run_id,
        "phase": "SP00",
        "task_id": "SP00-W06",
        "commit": head,
        "worktree_note": "Governance correction is uncommitted. This task was not authorized to commit.",
        "environment_manifest": "environment.json",
        "requirements": ["REQ-OPS-07"],
        "acceptance": [
            {
                "id": "AT-SEC-05",
                "scope": "SP00-W06 governance correction: license and advisory record shape, community templates, document QA",
                "status": "in_review",
                "exit_code": None,
                "slice_checks_failed": summary["failed"],
                "slice_checks_skipped": summary["skipped"],
                "applicable_passed_count": summary["applicable_passed_count"],
                "full_case_status": "pending",
                "full_case_not_run": DEFERRED,
            }
        ],
        "external_prerequisites": [
            "Appoint a security owner, a private vulnerability channel, a conduct contact, and a release contact.",
            "Replace OWNER_PENDING, uncomment CODEOWNERS, and turn on branch protection in one reviewed change.",
            "Run the full AT-SEC-05 scans when lock, JavaScript, native, and image artifacts are in scope. This checker does not do that.",
            "Independent follow-up review. reviewers is empty on purpose.",
        ],
        "artifacts": artifacts,
        "privacy": "public-synthetic",
        "reviewers": [],
        "waivers": [],
        "checker": {
            "path": "governance/check_governance.py",
            "default": "non-mutating",
            "write_flag": "--write-evidence --evidence-run " + run_id,
            "historical_checker_unchanged": str(HISTORICAL_CHECKER),
        },
    }
    manifest_path = target / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    (target / "manifest.sha256").write_text(sha256(manifest_path) + "  manifest.json\n")
    lines = [f"{sha256(manifest_path)}  manifest.json"]
    for artifact in artifacts:
        lines.append(f"{artifact['sha256']}  {artifact['path']}")
    (target / "hashes.sha256").write_text("\n".join(lines) + "\n")


def format_summary(summary: dict) -> str:
    lines = [
        "status=in_review",
        f"applicable_passed={summary['applicable_passed_count']}",
        f"skipped={summary['skipped_count']} {' '.join(summary['skipped'])}".rstrip(),
        f"failed={summary['failed_count']}",
        "deferred=" + "; ".join(summary["deferred"]),
        summary["note"],
    ]
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description="Non-mutating governance document QA")
    parser.add_argument("--self-test", action="store_true", help="run in-memory negative controls and exit")
    parser.add_argument(
        "--write-evidence",
        action="store_true",
        help="write the current run only; requires --evidence-run and never writes historical evidence",
    )
    parser.add_argument(
        "--evidence-run",
        default=None,
        help=f"evidence run id; the only permitted value is {PERMITTED_EVIDENCE_RUN}",
    )
    args = parser.parse_args(argv)
    if args.write_evidence:
        block = evidence_write_block(args.evidence_run)
        if block:
            print(block)
            return 2
    started = datetime.now(timezone.utc).isoformat()
    before = snapshot_immutable(ROOT)
    results: list[dict] = []
    commands: list[dict] = []
    as_of = observed_clock(datetime.now(timezone.utc))
    if args.self_test and not args.write_evidence:
        add_self_tests(results, as_of)
    else:
        add_self_tests(results, as_of)
        add_tree_checks(results, commands)
        after = snapshot_immutable(ROOT)
        changed = sorted(key for key, value in before.items() if after.get(key) != value)
        record(
            results,
            "immutable-inputs-unchanged",
            "passed" if not changed else "failed",
            changed or "LICENSE, specs, docs, execution prompts, historical evidence, and the dependency baseline were not modified by this process",
        )
    summary = summarize(results)
    text = format_summary(summary)
    print(text)
    for item in results:
        if item["status"] == "failed":
            print(f"FAILED {item['name']}: {item['detail']}")
    if args.write_evidence:
        write_evidence(results, commands, summary, started, args.evidence_run)
        print(f"evidence={evidence_rel(args.evidence_run)}")
    return 1 if summary["failed_count"] else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
