#!/usr/bin/env python3
"""SP00-W06 governance file checks.

Validates license bytes, community templates, and the supply-chain record
shape. Does not scan a lockfile, image, or product artifact, and does not
mark AT-SEC-05 passed.
"""

from __future__ import annotations

import hashlib
import json
import re
import shutil
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
EVIDENCE = Path(__file__).resolve().parent
LICENSE_SHA256 = "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"
RUN_ID = "eef831aa-dba4-4af2-a78b-541bc9e0a002"

GOVERNANCE_FILES = [
    "LICENSE",
    "NOTICE",
    "README.md",
    "CONTRIBUTING.md",
    "CODE_OF_CONDUCT.md",
    "SECURITY.md",
    "SUPPORT.md",
    ".github/CODEOWNERS",
    ".github/pull_request_template.md",
    ".github/ISSUE_TEMPLATE/config.yml",
    ".github/ISSUE_TEMPLATE/bug.yml",
    ".github/ISSUE_TEMPLATE/task.yml",
    ".github/ISSUE_TEMPLATE/dependency.yml",
    "governance/README.md",
    "governance/adr-template.md",
    "governance/ownership-map.md",
    "governance/review-guidance.md",
    "governance/supported-matrix.md",
    "supply-chain/policy.md",
    "supply-chain/dependency-audit-record.template.json",
]

PROHIBITED_LICENSES = {
    "AGPL-3.0-only",
    "AGPL-3.0-or-later",
    "AGPL-3.0",
    "SSPL-1.0",
    "BUSL-1.1",
    "Elastic-2.0",
}

EMAIL_RE = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}")
LINK_RE = re.compile(r"\[[^\]]+\]\(([^)\s]+)\)")
FORM_TYPES = {"markdown", "textarea", "input", "dropdown", "checkboxes"}
FORBIDDEN_CLAIMS = (
    "production-ready",
    "production ready",
    "ready for production",
    "all tests pass",
    "gvisor is installed",
    "gvisor enabled",
    "cdc is running",
    "cdc works",
    "mcp server is available",
    "mcp is available",
    "all dependencies are compliant",
    "every dependency is apache",
)


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    digest.update(path.read_bytes())
    return digest.hexdigest()


def strip_fences(text: str) -> str:
    kept: list[str] = []
    fenced = False
    for line in text.splitlines(True):
        if line.lstrip().startswith("```"):
            fenced = not fenced
            continue
        if not fenced:
            kept.append(line)
    return "".join(kept)


def whitespace_errors(path: Path) -> list[str]:
    errors: list[str] = []
    data = path.read_bytes()
    if b"\r" in data:
        errors.append(f"{path}: CR byte")
    if b"\0" in data:
        errors.append(f"{path}: NUL byte")
    for number, line in enumerate(data.splitlines(), 1):
        if line.endswith(b" ") or line.endswith(b"\t"):
            errors.append(f"{path}:{number}: trailing whitespace")
        if line.startswith(b"<<<<<<<") or line.startswith(b">>>>>>>") or line == b"=======":
            errors.append(f"{path}:{number}: conflict marker")
    return errors


def codeowners_active_lines(text: str) -> list[int]:
    active: list[int] = []
    for number, line in enumerate(text.splitlines(), 1):
        stripped = line.strip()
        if stripped and not stripped.startswith("#"):
            active.append(number)
    return active


def template_errors(doc: dict) -> list[str]:
    errors: list[str] = []
    if doc.get("schema_version") != 1:
        errors.append("schema_version")
    if doc.get("template") is not True:
        errors.append("template flag")
    if doc.get("audited") is not False:
        errors.append("audited must be false on the blank template")
    if doc.get("record_status") != "blank-template":
        errors.append("record_status")
    if doc.get("do_not_treat_as_baseline") is not True:
        errors.append("do_not_treat_as_baseline")
    if doc.get("exception") is not None:
        errors.append("exception must be null")
    if doc.get("baseline_file") != "supply-chain/dependency-baseline.json":
        errors.append("baseline_file")
    package = doc.get("package")
    if not isinstance(package, dict):
        return errors + ["package"]
    for key in ("name", "version", "license_spdx", "official_source", "digest", "publication_date"):
        if package.get(key) is not None:
            errors.append(f"blank package.{key}")
    if doc.get("reviewers_assigned") != []:
        errors.append("reviewers_assigned")
    return errors


def audited_record_errors(doc: dict) -> list[str]:
    errors: list[str] = []
    if doc.get("audited") is not True or doc.get("template") is not False:
        errors.append("audited record flags")
    package = doc.get("package")
    if not isinstance(package, dict):
        return ["package"]
    for key in (
        "ecosystem",
        "name",
        "version",
        "official_source",
        "repository",
        "publication_date",
        "adoption_date",
        "msrv",
        "license_spdx",
        "digest",
    ):
        if not package.get(key):
            errors.append(f"missing {key}")
    if not package.get("license_evidence"):
        errors.append("license_evidence")
    if not package.get("manifests"):
        errors.append("manifests")
    if not isinstance(package.get("features_enabled"), list):
        errors.append("features_enabled")
    if not isinstance(package.get("native_transitive"), list):
        errors.append("native_transitive")
    if package.get("prohibited_license") is not False:
        errors.append("prohibited_license flag")
    licenses = [package.get("license_spdx")]
    for native in package.get("native_transitive") or []:
        if isinstance(native, dict):
            licenses.append(native.get("license_spdx"))
    if any(item in PROHIBITED_LICENSES for item in licenses):
        errors.append("prohibited license")
    rule = package.get("seven_day_rule")
    age = package.get("age_days_at_adoption")
    if rule == "pass":
        if not isinstance(age, int) or age < 7:
            errors.append("age under 7")
        if doc.get("exception") is not None:
            errors.append("exception present on a passing age rule")
    elif rule == "security-exception":
        exception = doc.get("exception") or {}
        if exception.get("kind") != "security-fix-seven-day":
            errors.append("exception kind")
        for key in ("owner_role", "reason", "regression_evidence"):
            if not exception.get(key):
                errors.append(f"exception.{key}")
        if exception.get("waive"):
            errors.append("exception.waive")
    else:
        errors.append("seven_day_rule")
    return errors


def synthetic_audited(**overrides: object) -> dict:
    package = {
        "ecosystem": "cargo",
        "name": "example-synthetic-crate",
        "version": "1.2.3",
        "official_source": "https://example.invalid/example-synthetic-crate",
        "repository": "https://example.invalid/example-synthetic-crate.git",
        "manifests": ["crates/oo-core/Cargo.toml"],
        "publication_date": "2026-01-01",
        "adoption_date": "2026-02-01",
        "age_days_at_adoption": 31,
        "seven_day_rule": "pass",
        "msrv": "1.94.0",
        "features_enabled": [],
        "default_features_disabled": True,
        "license_spdx": "MIT",
        "license_evidence": ["LICENSE file in the crate, synthetic check only"],
        "dual_license_choice": None,
        "native_transitive": [],
        "digest": "sha256:" + ("ab" * 32),
        "prohibited_license": False,
    }
    document = {
        "template": False,
        "audited": True,
        "package": package,
        "exception": None,
    }
    package_overrides = overrides.pop("package", {})
    if isinstance(package_overrides, dict):
        package.update(package_overrides)
    document.update(overrides)
    return document


def claim_hits(text: str) -> list[str]:
    lowered = text.lower()
    return [phrase for phrase in FORBIDDEN_CLAIMS if phrase in lowered]


def local_links(path: Path) -> list[str]:
    text = strip_fences(path.read_text())
    missing: list[str] = []
    for target in LINK_RE.findall(text):
        if target.startswith(("http://", "https://", "mailto:")):
            continue
        cleaned = target.split("#", 1)[0]
        if not cleaned:
            continue
        resolved = (path.parent / cleaned).resolve()
        if not resolved.is_file():
            missing.append(f"{path}: {target}")
    return missing


def form_errors(path: Path, document: object) -> list[str]:
    errors: list[str] = []
    if not isinstance(document, dict):
        return [f"{path}: root is not a mapping"]
    if path.name == "config.yml":
        if document.get("blank_issues_enabled") is not False:
            errors.append(f"{path}: blank_issues_enabled")
        if "contact_links" in document:
            errors.append(f"{path}: contact_links would need a real URL")
        return errors
    for key in ("name", "description", "body"):
        if key not in document:
            errors.append(f"{path}: missing {key}")
    body = document.get("body")
    if not isinstance(body, list) or not body:
        return errors + [f"{path}: body"]
    seen: set[str] = set()
    for index, item in enumerate(body):
        if not isinstance(item, dict):
            errors.append(f"{path}: body[{index}]")
            continue
        kind = item.get("type")
        if kind not in FORM_TYPES:
            errors.append(f"{path}: body[{index}] type {kind}")
            continue
        if kind == "markdown":
            value = (item.get("attributes") or {}).get("value")
            if not isinstance(value, str) or not value.strip():
                errors.append(f"{path}: empty markdown")
            continue
        item_id = item.get("id")
        if not isinstance(item_id, str) or not re.fullmatch(r"[A-Za-z0-9_-]+", item_id):
            errors.append(f"{path}: bad id at {index}")
        elif item_id in seen:
            errors.append(f"{path}: duplicate id {item_id}")
        else:
            seen.add(item_id)
        attributes = item.get("attributes") or {}
        if not attributes.get("label"):
            errors.append(f"{path}: {item_id} label")
        if kind == "dropdown" and not attributes.get("options"):
            errors.append(f"{path}: {item_id} options")
        if kind == "checkboxes":
            options = attributes.get("options")
            if not isinstance(options, list) or not options:
                errors.append(f"{path}: {item_id} options")
            elif not any(option.get("required") is True for option in options if isinstance(option, dict)):
                errors.append(f"{path}: {item_id} has no required confirmation")
    return errors


def run(command: list[str]) -> dict:
    completed = subprocess.run(command, cwd=ROOT, text=True, capture_output=True)
    return {
        "command": command,
        "exit_code": completed.returncode,
        "stdout": completed.stdout[-4000:],
        "stderr": completed.stderr[-4000:],
    }


def load_yaml(path: Path) -> object:
    completed = subprocess.run(
        ["ruby", "-ryaml", "-e", "require 'json'; print JSON.generate(YAML.safe_load(File.read(ARGV[0])))", str(path)],
        cwd=ROOT,
        text=True,
        capture_output=True,
    )
    if completed.returncode != 0:
        raise RuntimeError(completed.stderr.strip() or completed.stdout.strip())
    return json.loads(completed.stdout)


def main() -> int:
    started = datetime.now(timezone.utc).isoformat()
    results: list[dict] = []

    def check(name: str, ok: bool, detail: object) -> None:
        results.append({"name": name, "ok": ok, "detail": detail})

    license_path = ROOT / "LICENSE"
    license_hash = sha256(license_path)
    check("license-sha256", license_hash == LICENSE_SHA256, license_hash)

    notice = (ROOT / "NOTICE").read_text()
    check("notice-records-license-hash", LICENSE_SHA256 in notice, "hash cited" if LICENSE_SHA256 in notice else "missing")
    check("notice-separates-obligations", "Unverified components stay unverified." in notice, "separation sentence")
    check("notice-coc-license", "Creative Commons Attribution 4.0" in notice, "coc license")

    readme = (ROOT / "README.md").read_text()
    check("readme-definition", "not a knowledge graph" in readme and "不是知识图谱" in readme, "both languages")
    check("readme-planned-command-label", "Planned, not available" in readme, "planned section")
    check(
        "readme-available-commands",
        "python3 specs/scripts/check_spec.py --self-test" in readme
        and "python3 specs/scripts/check_plan.py --self-test" in readme
        and "git diff --check" in readme,
        "three current commands",
    )

    for relative in ("README.md", "NOTICE", "SECURITY.md", "SUPPORT.md", "CONTRIBUTING.md"):
        hits = claim_hits((ROOT / relative).read_text())
        check(f"no-overclaim:{relative}", not hits, hits)

    check(
        "claim-denylist-fires",
        claim_hits("This build is production-ready and all tests pass") == ["production-ready", "all tests pass"],
        "in-memory negative",
    )

    emails: list[str] = []
    whitespace: list[str] = []
    for relative in GOVERNANCE_FILES:
        path = ROOT / relative
        if not path.is_file():
            check(f"exists:{relative}", False, "missing")
            continue
        whitespace.extend(whitespace_errors(path))
        found = EMAIL_RE.findall(path.read_text(errors="replace"))
        if found:
            emails.append(f"{relative}: {found}")
    check("no-email-addresses", not emails, emails)
    check("no-trailing-whitespace", not whitespace, whitespace)

    codeowners = (ROOT / ".github/CODEOWNERS").read_text()
    active = codeowners_active_lines(codeowners)
    check("codeowners-inactive", not active, active)
    augmented_owners = codeowners_active_lines(codeowners + "\n* @not-a-real-user\n")
    check(
        "codeowners-negative-detects-owner",
        active == [] and len(augmented_owners) == 1,
        augmented_owners,
    )
    check("codeowners-points-at-map", "governance/ownership-map.md" in codeowners, "map path")

    conduct = (ROOT / "CODE_OF_CONDUCT.md").read_text()
    check("coc-pending-contact", "No community leader is appointed" in conduct, "pending sentence")
    check("coc-no-placeholder", "[INSERT CONTACT METHOD]" not in conduct, "placeholder removed")
    check("coc-attribution", "Contributor Covenant" in conduct and "version 2.1" in conduct, "attribution")

    security = (ROOT / "SECURITY.md").read_text()
    for phrase in (
        "release blocker",
        "Do not open a public issue",
        "REQ-SEC-02",
        "REQ-SEC-03",
        "does not dump the catalog",
        "hash chain",
    ):
        check(f"security-contains:{phrase}", phrase in security, phrase)

    policy = (ROOT / "supply-chain/policy.md").read_text()
    for phrase in (
        "at least 7 days",
        "AGPL",
        "BUSL-1.1",
        "Elastic-2.0",
        "SSPL-1.0",
        "build.rs",
        "proc-macro",
        "first M1 artifact",
        "cargo audit",
        "Planned — harness",
        "dependency-baseline.json",
    ):
        check(f"policy-contains:{phrase}", phrase in policy, phrase)

    template = json.loads((ROOT / "supply-chain/dependency-audit-record.template.json").read_text())
    blank_errors = template_errors(template)
    check("audit-template-blank", not blank_errors, blank_errors)

    positive = audited_record_errors(synthetic_audited())
    prohibited = audited_record_errors(
        synthetic_audited(package={"license_spdx": "AGPL-3.0-only", "prohibited_license": True})
    )
    young = audited_record_errors(synthetic_audited(package={"age_days_at_adoption": 1}))
    waived = audited_record_errors(
        synthetic_audited(
            package={"age_days_at_adoption": 1, "seven_day_rule": "security-exception"},
            exception={
                "kind": "security-fix-seven-day",
                "owner_role": "security-owner",
                "reason": "synthetic negative",
                "regression_evidence": "evidence/none",
                "waive": ["authorization"],
            },
        )
    )
    unowned = audited_record_errors(
        synthetic_audited(
            package={"age_days_at_adoption": 1, "seven_day_rule": "security-exception"},
            exception={
                "kind": "security-fix-seven-day",
                "owner_role": "",
                "reason": "synthetic negative",
                "regression_evidence": "evidence/none",
                "waive": [],
            },
        )
    )
    check("audit-synthetic-pass-is-accepted-by-shape", not positive, positive)
    check("audit-negative-prohibited-license", "prohibited license" in prohibited, prohibited)
    check("audit-negative-young-version", "age under 7" in young, young)
    check("audit-negative-waive-authorization", "exception.waive" in waived, waived)
    check("audit-negative-blank-exception-owner", "exception.owner_role" in unowned, unowned)

    link_errors: list[str] = []
    for relative in GOVERNANCE_FILES:
        path = ROOT / relative
        if path.suffix.lower() in {".md", ".yml", ".yaml"} or path.name == "CODEOWNERS":
            if path.suffix.lower() == ".md":
                link_errors.extend(local_links(path))
    check("relative-links", not link_errors, link_errors)
    check(
        "link-checker-negative",
        local_links_from_text("See [missing](does-not-exist.md).\n", ROOT / "README.md") == ["does-not-exist.md"],
        "in-memory missing link",
    )

    yaml_details: list[dict] = []
    yaml_ok = True
    for relative in (
        ".github/ISSUE_TEMPLATE/config.yml",
        ".github/ISSUE_TEMPLATE/bug.yml",
        ".github/ISSUE_TEMPLATE/task.yml",
        ".github/ISSUE_TEMPLATE/dependency.yml",
    ):
        path = ROOT / relative
        try:
            document = load_yaml(path)
            errors = form_errors(path, document)
        except (RuntimeError, json.JSONDecodeError) as exc:
            document = None
            errors = [str(exc)]
        yaml_ok = yaml_ok and not errors
        yaml_details.append({"file": relative, "errors": errors})
    check("issue-form-schema", yaml_ok, yaml_details)

    yamllint = shutil.which("yamllint")
    yamllint_note = "not on PATH"
    if yamllint:
        try:
            probe = subprocess.run([yamllint, "--version"], cwd=ROOT, text=True, capture_output=True)
            yamllint_note = (probe.stdout or probe.stderr).strip()
            if probe.returncode != 0:
                yamllint = ""
        except OSError as exc:
            yamllint_note = f"present but not executable: {exc}"
            yamllint = ""
    if yamllint:
        yamllint_result = run(
            [
                yamllint,
                "-d",
                "{extends: default, rules: {line-length: {max: 120}, truthy: {check-keys: false}}}",
                ".github/ISSUE_TEMPLATE/config.yml",
                ".github/ISSUE_TEMPLATE/bug.yml",
                ".github/ISSUE_TEMPLATE/task.yml",
                ".github/ISSUE_TEMPLATE/dependency.yml",
            ]
        )
        check("yamllint", yamllint_result["exit_code"] == 0, yamllint_result)
    else:
        # Recorded skip. Ruby YAML.safe_load plus the form schema check is the parse that ran.
        results.append({
            "name": "yamllint",
            "ok": True,
            "skipped": True,
            "detail": yamllint_note + "; issue forms were parsed with Ruby YAML.safe_load",
        })

    spec = run([sys.executable, "specs/scripts/check_spec.py", "--self-test"])
    plan = run([sys.executable, "specs/scripts/check_plan.py", "--self-test"])
    diff_check = run(["git", "diff", "--check"])
    check("check-spec-self-test", spec["exit_code"] == 0, spec)
    check("check-plan-self-test", plan["exit_code"] == 0, plan)
    check("git-diff-check", diff_check["exit_code"] == 0, diff_check)

    baseline = ROOT / "supply-chain/dependency-baseline.json"
    if baseline.exists():
        baseline_detail = {"present": True, "sha256": sha256(baseline), "bytes": baseline.stat().st_size}
    else:
        baseline_detail = {"present": False}
    check("baseline-observed-not-written", True, baseline_detail)

    head = run(["git", "rev-parse", "HEAD"])
    commit = head["stdout"].strip()
    check("git-head", head["exit_code"] == 0 and len(commit) == 40, commit)

    ruby = run(["ruby", "-v"])
    environment = {
        "started": started,
        "finished": datetime.now(timezone.utc).isoformat(),
        "python": sys.version.split()[0],
        "ruby": ruby["stdout"].strip(),
        "yamllint": yamllint or "",
        "platform": sys.platform,
        "git_head": commit,
        "run_id": RUN_ID,
        "privacy": "public-synthetic",
    }
    (EVIDENCE / "environment.json").write_text(json.dumps(environment, indent=2) + "\n")
    (EVIDENCE / "checks.json").write_text(json.dumps({"results": results}, indent=2) + "\n")

    failed = [item["name"] for item in results if not item["ok"]]
    artifacts = []
    for relative in GOVERNANCE_FILES + [
        "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002/SP00/SP00-W06/check_sp00_w06.py",
        "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002/SP00/SP00-W06/checks.json",
        "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a002/SP00/SP00-W06/environment.json",
    ]:
        path = ROOT / relative
        artifacts.append({"path": relative, "sha256": sha256(path), "bytes": path.stat().st_size})

    manifest = {
        "spec_id": "OO-SPEC-001",
        "spec_version": "1.0.0",
        "plan_id": "OO-PLAN-001",
        "plan_version": "1.0.0",
        "run_id": RUN_ID,
        "phase": "SP00",
        "task_id": "SP00-W06",
        "commit": commit,
        "worktree_note": "Governance files are uncommitted. This task was not authorized to commit.",
        "environment_manifest": "environment.json",
        "requirements": ["REQ-OPS-07"],
        "acceptance": [
            {
                "id": "AT-SEC-05",
                "scope": "SP00-W06 foundation slice: project license, community templates, supply-chain policy contract",
                "status": "in_review",
                "exit_code": None,
                "slice_checks_failed": failed,
                "full_case_status": "pending",
                "full_case_not_run": [
                    "Cargo.lock / JavaScript lock / native / image license and vulnerability scan",
                    "offline signature tamper rejection",
                    "secret scan of a distributable artifact",
                    "SPDX SBOM and signature",
                    "dependency-baseline.json audit",
                ],
            }
        ],
        "artifacts": artifacts,
        "privacy": "public-synthetic",
        "reviewers": [],
        "waivers": [],
    }
    manifest_path = EVIDENCE / "manifest.json"
    manifest_path.write_text(json.dumps(manifest, indent=2) + "\n")
    (EVIDENCE / "manifest.sha256").write_text(sha256(manifest_path) + "  manifest.json\n")

    print(f"checks={len(results)} failed={len(failed)}")
    for item in results:
        if not item["ok"]:
            print(f"FAILED {item['name']}: {item['detail']}")
    return 1 if failed else 0


def local_links_from_text(text: str, path: Path) -> list[str]:
    missing: list[str] = []
    for target in LINK_RE.findall(strip_fences(text)):
        cleaned = target.split("#", 1)[0]
        if not cleaned or target.startswith(("http://", "https://")):
            continue
        if not (path.parent / cleaned).resolve().is_file():
            missing.append(cleaned)
    return missing


if __name__ == "__main__":
    sys.exit(main())
