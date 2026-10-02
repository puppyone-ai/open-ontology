#!/usr/bin/env python3
"""Independent attribution checks for commit fb2d1d19. Writes hashes and booleans."""
import hashlib
import json
import subprocess
import tarfile
import urllib.request
from io import BytesIO
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
COMMIT = "fb2d1d19c21b510dfd6231c3b67d0efbf4285491"
PARENT = "6fa6481267e5a4efac20831fa0b407a8cf028905"
B2 = "b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74"
PROBE = Path("/tmp/oo-a078-4ebcaafc-8442-4e95-9706-59ca03b0b1cb/upstream")
OUT_DIR = REPO / "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a078/SP00/isc-attribution-review/raw"
UA = "oo-a078-attribution-review (local byte check)"

CRATES = [
    ("r-efi", "6.0.0", "r-efi-6.0.0.crate"),
    ("r-efi", "5.3.0", "r-efi-5.3.0.crate"),
    ("rustls", "0.23.45", "rustls-0.23.45.crate"),
]


def git_show(spec: str) -> bytes:
    proc = subprocess.run(
        ["git", "-C", str(REPO), "show", spec],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if proc.returncode != 0:
        raise SystemExit(f"git show {spec} exit {proc.returncode}: {proc.stderr.decode()[:200]}")
    return proc.stdout


def git_rev(spec: str) -> str:
    proc = subprocess.run(
        ["git", "-C", str(REPO), "rev-parse", spec],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        text=True,
    )
    if proc.returncode != 0:
        return f"EXIT {proc.returncode} {proc.stderr.strip()}"
    return proc.stdout.strip()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def fetch(url: str, dest: Path) -> int:
    req = urllib.request.Request(url, headers={"User-Agent": UA})
    try:
        with urllib.request.urlopen(req, timeout=60) as resp:
            body = resp.read()
            code = getattr(resp, "status", 200)
        dest.write_bytes(body)
        (dest.with_suffix(dest.suffix + ".http")).write_text(str(code) + "\n")
        return 0
    except Exception as exc:
        dest.with_suffix(dest.suffix + ".err").write_text(type(exc).__name__ + "\n")
        return 1


def lock_packages(text: str):
    rows = []
    current = {}
    for line in text.splitlines():
        if line.strip() == "[[package]]":
            if current:
                rows.append(current)
            current = {}
            continue
        if " = " in line and not line.startswith(" "):
            key, value = line.split(" = ", 1)
            current[key] = value.strip().strip('"')
    if current:
        rows.append(current)
    return rows


def crate_files(blob: bytes):
    files = {}
    with tarfile.open(fileobj=BytesIO(blob), mode="r:gz") as tar:
        for member in tar.getmembers():
            if not member.isfile():
                continue
            extracted = tar.extractfile(member)
            if extracted is None:
                continue
            files[member.name] = extracted.read()
    return files


def section(text: str, start: str, end_markers):
    idx = text.find(start)
    if idx < 0:
        return ""
    rest = text[idx + len(start):]
    end = len(rest)
    for marker in end_markers:
        pos = rest.find(marker)
        if pos >= 0:
            end = min(end, pos)
    return rest[:end]


def main():
    PROBE.mkdir(parents=True, exist_ok=True)
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    commands = []
    result = {"commit": COMMIT, "parent": PARENT, "tree": git_rev(f"{COMMIT}^{{tree}}")}

    fetches = []
    for name, version, filename in CRATES:
        crate_url = f"https://static.crates.io/crates/{name}/{filename}"
        api_url = f"https://crates.io/api/v1/crates/{name}/{version}"
        crate_path = PROBE / filename
        api_path = PROBE / f"{name}-{version}.api.json"
        crate_exit = fetch(crate_url, crate_path)
        api_exit = fetch(api_url, api_path)
        commands.append({"argv": ["GET", crate_url], "exit": crate_exit, "bytes": crate_path.stat().st_size if crate_path.exists() else 0})
        commands.append({"argv": ["GET", api_url], "exit": api_exit, "bytes": api_path.stat().st_size if api_path.exists() else 0})
        fetches.append((name, version, filename, crate_path, api_path, crate_exit, api_exit))

    b2_lock = git_show(f"{B2}:Cargo.lock").decode()
    head_lock = git_show(f"{COMMIT}:Cargo.lock").decode()
    b2_rows = {(r.get("name"), r.get("version")): r.get("checksum") for r in lock_packages(b2_lock)}
    head_rows = {(r.get("name"), r.get("version")): r.get("checksum") for r in lock_packages(head_lock)}

    crate_reports = []
    extracted = {}
    for name, version, filename, crate_path, api_path, crate_exit, api_exit in fetches:
        report = {"name": name, "version": version, "crate_fetch_exit": crate_exit, "api_fetch_exit": api_exit}
        if crate_exit == 0:
            blob = crate_path.read_bytes()
            report["bytes"] = len(blob)
            report["sha256"] = sha256(blob)
            files = crate_files(blob)
            extracted[(name, version)] = files
            basenames = {}
            for path, data in files.items():
                base = path.rsplit("/", 1)[-1]
                basenames.setdefault(base, 0)
                basenames[base] += 1
            report["file_count"] = len(files)
            report["has_LICENSE"] = "LICENSE" in basenames
            report["has_COPYING"] = "COPYING" in basenames
            report["has_NOTICE"] = "NOTICE" in basenames
            report["basename_LICENSE_COPYING_NOTICE"] = {
                key: basenames.get(key, 0) for key in ("LICENSE", "COPYING", "NOTICE")
            }
        if api_exit == 0:
            api = json.loads(api_path.read_text())
            version_row = api.get("version") or {}
            report["api_checksum"] = version_row.get("checksum")
            report["api_yanked"] = version_row.get("yanked")
            report["api_created_at"] = version_row.get("created_at")
            report["api_license"] = version_row.get("license")
            report["sha256_equals_api_checksum"] = report.get("sha256") == version_row.get("checksum")
        report["b2_lock_checksum"] = b2_rows.get((name, version))
        report["head_lock_checksum"] = head_rows.get((name, version))
        report["sha256_equals_b2_lock"] = report.get("sha256") == report["b2_lock_checksum"]
        report["present_in_reviewed_root_lock"] = (name, version) in head_rows
        crate_reports.append(report)
    result["crates"] = crate_reports

    def file_of(name, version, suffix):
        files = extracted[(name, version)]
        matches = [data for path, data in files.items() if path.endswith("/" + suffix) or path.endswith(suffix)]
        # prefer exact basename
        exact = [data for path, data in files.items() if path.rsplit("/", 1)[-1] == suffix]
        return exact[0] if exact else None

    authors_60 = file_of("r-efi", "6.0.0", "AUTHORS")
    authors_53 = file_of("r-efi", "5.3.0", "AUTHORS")
    readme_60 = file_of("r-efi", "6.0.0", "README.md")
    readme_53 = file_of("r-efi", "5.3.0", "README.md")
    isc = file_of("rustls", "0.23.45", "LICENSE-ISC")
    mit = file_of("rustls", "0.23.45", "LICENSE-MIT")

    bodies = {
        "r-efi-6.0.0/AUTHORS": git_show(f"{COMMIT}:contracts/supply-chain/isc/bodies/r-efi-6.0.0/AUTHORS"),
        "r-efi-5.3.0/AUTHORS": git_show(f"{COMMIT}:contracts/supply-chain/isc/bodies/r-efi-5.3.0/AUTHORS"),
        "rustls LICENSE-ISC": git_show(f"{COMMIT}:contracts/supply-chain/isc/bodies/rustls-0.23.45/LICENSE-ISC"),
    }
    upstream_copies = {
        "r-efi-6.0.0/AUTHORS": git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/upstream/r-efi-6.0.0.AUTHORS"),
        "r-efi-5.3.0/AUTHORS": git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/upstream/r-efi-5.3.0.AUTHORS"),
        "rustls LICENSE-ISC": git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/upstream/rustls-0.23.45.LICENSE-ISC"),
        "rustls LICENSE-MIT": git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/upstream/rustls-0.23.45.LICENSE-MIT"),
    }

    def describe_authors(data: bytes):
        text = data.decode()
        mit = section(text, "AUTHORS-MIT:\n", ["AUTHORS-ASL:", "AUTHORS-LGPL:", "COPYRIGHT:", "AUTHORS:"])
        asl = section(text, "AUTHORS-ASL:\n", ["AUTHORS-LGPL:", "COPYRIGHT:", "AUTHORS:"])
        lgpl = section(text, "AUTHORS-LGPL:\n", ["COPYRIGHT:", "AUTHORS:"])
        grant_at = text.find("Permission is hereby granted, free of charge")
        copy_at = text.find("Copyright (C) 2017-2023 Red Hat, Inc.")
        names_at = text.find("AUTHORS: (ordered alphabetically)")
        name_block = text[names_at:] if names_at >= 0 else ""
        names = [line.strip() for line in name_block.splitlines() if line.startswith("        ") and "<" in line]
        return {
            "bytes": len(data),
            "sha256": sha256(data),
            "has_full_mit_grant": "Permission is hereby granted, free of charge" in text and "The above copyright notice and this permission notice shall be included" in text,
            "copyright_lines": [
                "Copyright (C) 2017-2023 Red Hat, Inc." in text,
                "Copyright (C) 2019-2023 Microsoft Corporation" in text,
                "Copyright (C) 2022-2023 David Rheinsberg" in text,
            ],
            "copyright_below_grant": grant_at >= 0 and copy_at > grant_at,
            "name_list_count": len(names),
            "name_list_below_copyright": names_at > copy_at > 0,
            "contains_alan_egerton": "Alan Egerton <eggyal@gmail.com>" in text,
            "asl_has_apache_url": "http://www.apache.org/licenses/LICENSE-2.0" in asl,
            "asl_is_not_full_apache": "Grant of Copyright License" not in asl and "APPENDIX: How to apply" not in asl and len(asl) < 800,
            "lgpl_has_gnu_url": "http://www.gnu.org/licenses/" in lgpl,
            "lgpl_is_not_full_lgpl": "0. Additional Definitions" not in lgpl and "TERMS AND CONDITIONS" not in lgpl and len(lgpl) < 1200,
            "readme_is_not_this_file": True,
        }

    authors_diff = None
    if authors_60 and authors_53:
        lines_60 = authors_60.decode().splitlines()
        lines_53 = authors_53.decode().splitlines()
        only_60 = [line for line in lines_60 if line not in lines_53]
        only_53 = [line for line in lines_53 if line not in lines_60]
        authors_diff = {"only_in_6_0_0": only_60, "only_in_5_3_0": only_53}

    readme_report = None
    if readme_60 and readme_53:
        text = readme_60.decode()
        readme_report = {
            "sha256": sha256(readme_60),
            "byte_identical_5_3_0": readme_60 == readme_53,
            "spdx_markdown_line": "**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**" in text,
            "contains_mit_grant": "Permission is hereby granted, free of charge" in text,
        }

    isc_lines = isc.decode().splitlines() if isc else []
    mit_lines = mit.decode().splitlines() if mit else []
    rustls_report = {
        "license_isc_sha256": sha256(isc) if isc else None,
        "license_isc_bytes": len(isc) if isc else None,
        "license_isc_line2": isc_lines[1] if len(isc_lines) > 1 else None,
        "license_mit_line1": mit_lines[0] if mit_lines else None,
        "isc_has_comma": bool(isc_lines[1:2] and isc_lines[1] == "Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>"),
        "mit_has_no_comma": bool(mit_lines[:1] and mit_lines[0] == "Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>"),
    }

    result["authors_6_0_0"] = describe_authors(authors_60) if authors_60 else None
    result["authors_5_3_0"] = describe_authors(authors_53) if authors_53 else None
    result["authors_line_diff"] = authors_diff
    result["readme"] = readme_report
    result["rustls"] = rustls_report
    result["body_matches_upstream_crate"] = {
        "r-efi-6.0.0 AUTHORS": bodies["r-efi-6.0.0/AUTHORS"] == authors_60,
        "r-efi-5.3.0 AUTHORS": bodies["r-efi-5.3.0/AUTHORS"] == authors_53,
        "rustls LICENSE-ISC": bodies["rustls LICENSE-ISC"] == isc,
    }
    result["a077_raw_matches_crate"] = {
        "r-efi-6.0.0 AUTHORS": upstream_copies["r-efi-6.0.0/AUTHORS"] == authors_60,
        "r-efi-5.3.0 AUTHORS": upstream_copies["r-efi-5.3.0/AUTHORS"] == authors_53,
        "rustls LICENSE-ISC": upstream_copies["rustls LICENSE-ISC"] == isc,
        "rustls LICENSE-MIT": upstream_copies["rustls LICENSE-MIT"] == mit,
    }
    result["body_sha256"] = {key: sha256(value) for key, value in bodies.items()}

    notice = git_show(f"{COMMIT}:contracts/supply-chain/isc/NOTICE.md").decode()
    policy = git_show(f"{COMMIT}:contracts/supply-chain/isc/policy.md").decode()
    provenance = git_show(f"{COMMIT}:contracts/supply-chain/isc/provenance.md").decode()
    erratum = git_show(f"{COMMIT}:contracts/supply-chain/isc/ERRATUM.md").decode()
    parent_notice = git_show(f"{PARENT}:contracts/supply-chain/isc/NOTICE.md").decode()
    parent_policy = git_show(f"{PARENT}:contracts/supply-chain/isc/policy.md").decode()
    parent_prov = git_show(f"{PARENT}:contracts/supply-chain/isc/provenance.md").decode()
    a067_report = git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067/SP00/isc-policy/REPORT.md").decode()
    a067_handoff = git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067/SP00/isc-policy/HANDOFF.md").decode()
    deny = git_show(f"{COMMIT}:ci/deny.toml").decode()
    supply_policy = git_show(f"{COMMIT}:supply-chain/policy.md").decode()

    wrong_notice = "`r-efi` 6.0.0 has no MIT license text in the crate. Its README names `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. A distribution that relies on MIT still owes the MIT notice duty and cannot point at a MIT license file in this crate. `AUTHORS` is not that license."
    wrong_policy_1 = "`r-efi` 6.0.0, `MIT OR Apache-2.0 OR LGPL-2.1-or-later`. The published crate has no `LICENSE` file. Deny accepts the expression. That acceptance does not mean the MIT text is in the crate. `r-efi` 5.3.0 declares the same expression and was not a license error. Its tree was not opened for this record."
    wrong_policy_2 = "`fixtures/mit-or-gpl-missing-mit-text` is the synthetic form of the `r-efi` fact. A deny exit of 0 is the expression decision. It is not a finding that the selected license text was shipped."
    wrong_handoff = "that is the `r-efi` 6.0.0 situation, and it is not proof the MIT text shipped."
    wrong_report = "`r-efi` 6.0.0 is the real crate with that gap."
    wrong_comma = "Copyright (c) 2016 Joseph Birr-Pixton"

    result["historical_unchanged"] = {
        "a067_tree_parent": git_rev(f"{PARENT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067"),
        "a067_tree_commit": git_rev(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a067"),
        "fixtures_tree_parent": git_rev(f"{PARENT}:contracts/supply-chain/isc/fixtures"),
        "fixtures_tree_commit": git_rev(f"{COMMIT}:contracts/supply-chain/isc/fixtures"),
        "deny_parent": git_rev(f"{PARENT}:ci/deny.toml"),
        "deny_commit": git_rev(f"{COMMIT}:ci/deny.toml"),
        "supply_policy_parent": git_rev(f"{PARENT}:supply-chain/policy.md"),
        "supply_policy_commit": git_rev(f"{COMMIT}:supply-chain/policy.md"),
        "cargo_lock_parent": git_rev(f"{PARENT}:Cargo.lock"),
        "cargo_lock_commit": git_rev(f"{COMMIT}:Cargo.lock"),
        "cargo_toml_parent": git_rev(f"{PARENT}:Cargo.toml"),
        "cargo_toml_commit": git_rev(f"{COMMIT}:Cargo.toml"),
    }
    result["wrong_wording_still_in_history"] = {
        "parent_notice": wrong_notice in parent_notice,
        "parent_policy_1": wrong_policy_1 in parent_policy,
        "parent_policy_2": wrong_policy_2 in parent_policy,
        "a067_handoff": wrong_handoff in a067_handoff,
        "a067_report_gap": wrong_report in a067_report,
        "a067_report_drops_isc_comma_gloss": wrong_comma in a067_report and "Copyright (c) 2016, Joseph Birr-Pixton" not in a067_report,
    }
    result["erratum_quotes_and_supersedes"] = {
        "says_supersedes": erratum.startswith("# Erratum") and "This file supersedes the statements quoted below." in erratum,
        "quotes_wrong_notice": wrong_notice in erratum,
        "quotes_wrong_policy_1": wrong_policy_1 in erratum,
        "quotes_wrong_policy_2": wrong_policy_2 in erratum,
        "quotes_wrong_handoff": wrong_handoff in erratum,
        "quotes_wrong_report": wrong_report in erratum,
        "quotes_isc_comma_line": "Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>" in erratum,
        "names_parent_record": "raw/parent.txt" in erratum,
        "does_not_name_own_commit": COMMIT not in erratum,
        "parent_txt": git_show(f"{COMMIT}:evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/parent.txt").decode().strip(),
    }
    result["current_text_does_not_repeat_wrong_claim"] = {
        "notice_has_no_mit_text_claim": wrong_notice not in notice and "has no MIT license text" not in notice,
        "policy_has_no_mit_text_claim": "does not mean the MIT text is in the crate" not in policy and "synthetic form of the `r-efi` fact" not in policy,
        "provenance_stopped_at_readme_only": "stopped the 6.0.0 description at the README" not in provenance,
        "notice_states_grant": "AUTHORS-MIT" in notice and "Permission is hereby granted, free of charge" in notice,
        "notice_states_three_copyrights": all(s in notice for s in ("Red Hat, Inc.", "Microsoft Corporation", "David Rheinsberg")),
        "notice_states_name_list_and_alan": "Alan Egerton <eggyal@gmail.com>" in notice and "name list" in notice,
        "notice_asl_lgpl_pointer_distinction": "not the full Apache-2.0 text" in notice and "not the full LGPL text" in notice,
        "policy_states_grant_and_both_versions": "full MIT grant" in policy and "5.3.0" in policy and "Alan Egerton <eggyal@gmail.com>" in policy,
        "policy_pending_or_mismatch": "stays explicit and separate pending work" in policy and "does not weaken either rule" in policy,
        "policy_cdla_not_allowed": "`CDLA-Permissive-2.0` is not allowed." in policy,
        "notice_cdla_not_allowed": "CDLA-Permissive-2.0" in notice and "is not allowed" in notice,
        "erratum_cdla_not_added": "does not add `CDLA-Permissive-2.0`" in erratum,
        "erratum_isc_stays": 'still allows `ISC`' in erratum,
        "ring_stays_and": "Apache-2.0 AND ISC" in notice and "Apache-2.0 AND ISC" in erratum,
        "provenance_both_versions_and_comma": "r-efi 5.3.0 `AUTHORS`" in provenance and "Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>" in provenance,
        "no_full_w06": "SP00-W06" in erratum and "stay unapproved" in erratum,
    }
    allow = []
    capture = False
    for line in deny.splitlines():
        if line.startswith("allow = ["):
            capture = True
        if capture:
            allow.append(line)
            if line.strip() == "]":
                break
    result["deny"] = {
        "allow_block": allow,
        "isc_count": deny.count('"ISC"'),
        "cdla_count": deny.count("CDLA"),
        "unchanged_blob": result["historical_unchanged"]["deny_parent"] == result["historical_unchanged"]["deny_commit"],
    }
    result["supply_chain_policy_or_lines"] = {
        "gpl_even_in_or": "even when an `OR` expression also names a permissive license" in supply_policy,
        "prohibited_token_one_side_of_or": "even when it is only one side of an `OR`" in supply_policy,
        "blob_unchanged": result["historical_unchanged"]["supply_policy_parent"] == result["historical_unchanged"]["supply_policy_commit"],
    }
    fixture = git_show(f"{COMMIT}:contracts/supply-chain/isc/fixtures/mit-or-gpl-missing-mit-text/LICENSE").decode()
    result["missing_text_fixture"] = {
        "tree_unchanged": result["historical_unchanged"]["fixtures_tree_parent"] == result["historical_unchanged"]["fixtures_tree_commit"],
        "license_says_neither_text": "MIT" in fixture and "GPL" in fixture and ("neither" in fixture.lower() or "not" in fixture.lower()),
        "license_text": fixture,
    }
    diff = subprocess.run(
        ["git", "-C", str(REPO), "diff", "--name-only", PARENT, COMMIT],
        stdout=subprocess.PIPE,
        text=True,
        check=False,
    )
    names = [line for line in diff.stdout.splitlines() if line]
    allowed_prefix = (
        "contracts/supply-chain/isc/",
        "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/",
    )
    result["diff"] = {
        "exit": diff.returncode,
        "path_count": len(names),
        "outside_owned": [path for path in names if not path.startswith(allowed_prefix)],
        "touches_deny": any(path == "ci/deny.toml" for path in names),
        "touches_a067": any("e0a067/" in path for path in names),
        "touches_root_manifest": any(path in ("Cargo.toml", "Cargo.lock") for path in names),
        "touches_supply_policy": any(path == "supply-chain/policy.md" for path in names),
    }
    result["commands"] = commands
    (OUT_DIR / "attribution-verify.json").write_text(json.dumps(result, indent=2) + "\n")
    summary = {
        "crate_sha_matches_api_and_b2": [
            {
                "name": row["name"],
                "version": row["version"],
                "sha256_equals_api_checksum": row.get("sha256_equals_api_checksum"),
                "sha256_equals_b2_lock": row.get("sha256_equals_b2_lock"),
                "yanked": row.get("api_yanked"),
                "in_reviewed_root_lock": row.get("present_in_reviewed_root_lock"),
            }
            for row in crate_reports
        ],
        "body_matches_upstream_crate": result["body_matches_upstream_crate"],
        "authors_line_diff": authors_diff,
        "readme_identical": None if not readme_report else readme_report["byte_identical_5_3_0"],
        "rustls_comma": rustls_report["isc_has_comma"],
        "historical_a067_same": result["historical_unchanged"]["a067_tree_parent"] == result["historical_unchanged"]["a067_tree_commit"],
        "fixtures_same": result["historical_unchanged"]["fixtures_tree_parent"] == result["historical_unchanged"]["fixtures_tree_commit"],
        "deny_same": result["deny"]["unchanged_blob"],
        "outside_owned": result["diff"]["outside_owned"],
        "current_flags": result["current_text_does_not_repeat_wrong_claim"],
        "wrong_still_in_history": result["wrong_wording_still_in_history"],
        "erratum": result["erratum_quotes_and_supersedes"],
    }
    print(json.dumps(summary, indent=2))


if __name__ == "__main__":
    main()
