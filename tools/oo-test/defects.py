"""Replay historical defects in an owned scratch tree, never the checkout."""
import argparse
from collections import Counter
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

import runner

CORPUS = Path(__file__).with_name("defects.json")


def scratch_path(scratch, name):
    path = Path(name)
    if path.is_absolute() or ".." in path.parts or ".git" in path.parts:
        raise ValueError("mutation_path_escape")
    target = scratch / path
    target.resolve().relative_to(scratch.resolve())
    return target


def replace_exact(path, before, after, count=1):
    text = path.read_text()
    if text.count(before) != count:
        raise ValueError("mutation_preimage_mismatch")
    path.write_text(text.replace(before, after))


def classify(returncode, output, detector, baseline=False):
    """Only a completed assertion failure is a catch, never a broken build."""
    if detector == "rust":
        matches = re.findall(
            r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored;", output)
        if not matches:
            return "error"
        passed = sum(int(m[1]) for m in matches)
        failed = sum(int(m[2]) for m in matches)
        if passed + failed == 0:
            return "error"
        if returncode == 0 and failed == 0:
            return "passed" if baseline else "missed"
        if returncode == 101 and failed and "error[E" not in output:
            return "baseline_failed" if baseline else "caught"
    elif detector == "python":
        if not re.search(r"Ran [1-9]\d* tests? in ", output):
            return "error"
        if returncode == 0 and re.search(r"\nOK\s*$", output):
            return "passed" if baseline else "missed"
        if returncode == 1 and re.search(r"FAILED \(failures=[1-9]\d*\)\s*$", output):
            return "baseline_failed" if baseline else "caught"
    return "error"


def execute(command, cwd, log, env, timeout):
    started = time.monotonic()
    with log.open("wb") as output:
        process = subprocess.Popen(command, cwd=cwd, env=env, stdout=output,
                                   stderr=subprocess.STDOUT, start_new_session=True)
        try:
            code = process.wait(timeout=timeout)
        except (subprocess.TimeoutExpired, KeyboardInterrupt):
            os.killpg(process.pid, signal.SIGTERM)
            try:
                process.wait(timeout=10)
            except subprocess.TimeoutExpired:
                os.killpg(process.pid, signal.SIGKILL)
                process.wait()
            raise
    return code, log.read_text(errors="replace"), time.monotonic() - started


def cleanup_live(cell):
    """Timeouts kill the test process, so its unittest cleanup may not run."""
    for receipt in cell.glob("*-live/gate-owner.json"):
        owner = json.loads(receipt.read_text())
        ids = subprocess.check_output(
            ["docker", "ps", "-aq", "--filter", "label=oo.lane=ta",
             "--filter", "label=oo.run_id=" + owner["run_id"]], text=True).split()
        for container in ids:
            info = json.loads(subprocess.check_output(
                ["docker", "inspect", container], text=True))[0]
            labels = info["Config"]["Labels"]
            if (info["Name"] != "/" + owner["name"] or labels.get("oo.lane") != "ta"
                    or labels.get("oo.run_id") != owner["run_id"]):
                raise RuntimeError("cleanup_ownership_mismatch")
            subprocess.run(["docker", "rm", "-f", "-v", container],
                           check=True, capture_output=True)


def build_env(target_dir):
    return dict(os.environ, PYTHONDONTWRITEBYTECODE="1", CARGO_BUILD_JOBS="4",
                CARGO_INCREMENTAL="0", CARGO_TARGET_DIR=str(target_dir),
                OO_TA_SNAPSHOT_CACHE=str(runner.ROOT / "evidence/ta-fixture-cache"))


def add_worktree(ref):
    # Scratch trees live under /tmp and are removed after the gate.
    path = Path(tempfile.mkdtemp(prefix="oo-ta-gate-", dir="/tmp")) / "tree"
    subprocess.run(["git", "worktree", "add", "--detach", str(path), ref],
                   cwd=runner.ROOT, check=True, capture_output=True)
    return path


def remove_worktree(path):
    result = subprocess.run(["git", "worktree", "remove", "--force", str(path)],
                            cwd=runner.ROOT, capture_output=True)
    shutil.rmtree(path.parent, ignore_errors=True)
    return result.returncode == 0


def overlay_detectors(scratch):
    """Detectors are tools/oo-test files under review, not repaired product files."""
    overlays = {}
    for name in ("defect_checks.py", "defect_live.py", "snapshots.py"):
        source = Path(__file__).with_name(name)
        if source.exists() and (scratch / "tools/oo-test").is_dir():
            shutil.copyfile(source, scratch / "tools/oo-test" / name)
            overlays[name] = hashlib.sha256(source.read_bytes()).hexdigest()
    return overlays


def apply_replacements(scratch, patches, original):
    for patch in patches:
        path = scratch_path(scratch, patch["path"])
        original.setdefault(path, path.read_bytes())
        replace_exact(path, patch["before"], patch["after"], patch.get("count", 1))


def replay(defect, scratch, cell, env, timeout, row):
    original = {}
    try:
        missing = [path for path in defect.get("required_paths", [])
                   if not scratch_path(scratch, path).is_file()]
        if missing:
            row.update(status="unavailable", reason="required_product_files_missing",
                       missing=missing)
            return
        for patch in defect.get("detector_patches", []):
            path = scratch_path(scratch, patch["path"])
            original.setdefault(path, path.read_bytes())
            path.write_text(path.read_text() + patch["append"])
        # A fix absent from this base is overlaid for the baseline; the mutant
        # is then the base's own code, so a catch also flags a live defect.
        apply_replacements(scratch, defect.get("baseline_patches", []), original)
        if defect.get("baseline_patches"):
            row["defect_present_at_base"] = True
        for phase in ("baseline", "mutant"):
            if phase == "mutant":
                apply_replacements(scratch, defect["mutations"], original)
                diff = subprocess.check_output(["git", "diff", "--no-ext-diff"], cwd=scratch)
                (cell / "mutation.patch").write_bytes(diff)
                row["patch_sha256"] = hashlib.sha256(diff).hexdigest()
            command = (["nice", "-n", "10"] + defect["command"]
                       if defect["command"][0] == "cargo" else defect["command"])
            code, output, elapsed = execute(
                command, scratch, cell / (phase + ".log"),
                dict(env, OO_TA_LIVE_EVIDENCE=str(cell / (phase + "-live"))), timeout)
            outcome = classify(code, output, defect["detector"], phase == "baseline")
            row[phase] = {"command": defect["command"], "exit": code,
                          "seconds": round(elapsed, 3), "outcome": outcome}
            row["status"] = outcome
            if phase == "baseline" and outcome != "passed":
                return
    except subprocess.TimeoutExpired:
        row.update(status="error", reason="timeout_not_a_catch")
    except Exception as error:
        row.update(status="error", reason=type(error).__name__)
    finally:
        for path, data in original.items():
            path.write_bytes(data)
        try:
            cleanup_live(cell)
        except Exception as error:
            row.update(status="error", cleanup_error=type(error).__name__)


def run_gate(root, corpus, only=None, timeout=1200):
    sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=runner.ROOT,
                                  text=True).strip()
    rows = []
    report = {"format": "oo-defects/v2", "sha": sha,
              "corpus_sha256": hashlib.sha256(CORPUS.read_bytes()).hexdigest(),
              "complete_corpus": only is None, "results": rows, "status": "failed"}
    trees = {}
    cleanup_ok = True
    try:
        for defect in corpus:
            if only and defect["id"] not in only:
                continue
            ref = defect.get("base_ref", sha)
            if ref not in trees:
                tree = add_worktree(ref)
                overlays = overlay_detectors(tree)
                report.setdefault("detector_sha256", {}).update(overlays)
                # Foreign bases build in their own target dir so HEAD's cache stays warm.
                target = (runner.ROOT / "target" if ref == sha
                          else tree.parent / "target")
                trees[ref] = (tree, build_env(target))
            tree, env = trees[ref]
            row = {"id": defect["id"], "fix": defect["fix"], "status": "error",
                   "base": "head" if ref == sha else ref,
                   "layer": defect.get("layer", "L0")}
            rows.append(row)
            cell = root / defect["id"]
            cell.mkdir()
            replay(defect, tree, cell, env, timeout, row)
            runner.write_json(root / "defects.json", report)
            print(json.dumps(row), flush=True)
    finally:
        for tree, _ in trees.values():
            cleanup_ok = remove_worktree(tree) and cleanup_ok
        report["cleanup_ok"] = cleanup_ok
        report["counts"] = dict(Counter(row["status"] for row in rows))
        report["status"] = ("passed" if rows and only is None and cleanup_ok
                            and all(row["status"] == "caught" for row in rows) else "failed")
        runner.write_json(root / "defects.json", report)
        (root / "table.md").write_text(table(rows))
    return report


def table(rows):
    lines = ["| Defect | Fix | Base | Layer | Baseline | Mutant | Verdict |",
             "| --- | --- | --- | --- | --- | --- | --- |"]
    for row in rows:
        base = row["base"] + (" (+fix overlay)" if row.get("defect_present_at_base") else "")
        lines.append("| {} | {} | {} | {} | {} | {} | {} |".format(
            row["id"], row["fix"], base, row["layer"],
            (row.get("baseline") or {}).get("outcome", "-"),
            (row.get("mutant") or {}).get("outcome", "-"), row["status"]))
    return "\n".join(lines) + "\n"


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--defects", action="store_true", required=True)
    parser.add_argument("--run-id", required=True)
    parser.add_argument("--only", help="diagnostic subset; cannot pass the complete gate")
    parser.add_argument("--timeout", type=int, default=1200)
    args = parser.parse_args(argv)
    if not re.fullmatch(r"[a-z0-9][a-z0-9-]{0,100}", args.run_id) or args.timeout < 1:
        parser.error("invalid run id or timeout")
    corpus = json.loads(CORPUS.read_text())
    selected = set(args.only.split(",")) if args.only else None
    if selected and not selected <= {item["id"] for item in corpus}:
        parser.error("unknown defect id")
    root = runner.ROOT / "evidence" / args.run_id
    root.mkdir(mode=0o700, parents=True, exist_ok=False)
    report = run_gate(root, corpus, selected, args.timeout)
    print(json.dumps({k: v for k, v in report.items() if k != "results"}))
    return 0 if report["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
