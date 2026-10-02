#!/usr/bin/env python3
"""Count credential-shaped material without recording match text or match hashes.

Reads git objects, the committed a077 evidence tree, the working evidence tree,
and named /tmp leftovers. Writes aggregate counts and safe paths only.
"""
import hashlib
import json
import os
import re
import subprocess
import sys
from collections import defaultdict
from pathlib import Path

REPO = Path("/Users/redux/puppy-one/open-ontology")
COMMIT = "fb2d1d19c21b510dfd6231c3b67d0efbf4285491"
OUT = REPO / "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a078/SP00/isc-attribution-review/raw/exposure-scan.json"
MAX_BLOB = 20 * 1024 * 1024

PATTERNS = {
    "cursor_key_prefix": re.compile(br"key_[A-Za-z0-9]{16,}"),
    "crsr_prefix": re.compile(br"crsr_[A-Za-z0-9]{16,}"),
    "workos_session_cookie": re.compile(br"WorkosCursorSessionToken"),
    "workos_user_session": re.compile(br"user_[A-Za-z0-9]{20,}::"),
    "jwt": re.compile(br"eyJ[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}\.[A-Za-z0-9_-]{8,}"),
    "cursor_api_assignment": re.compile(br"(?i)cursor[^\n\r]{0,40}(api[_-]?key|session)[^\n\r]{0,20}[=:][^\n\r]{0,8}[A-Za-z0-9_\-]{12,}"),
    "bearer": re.compile(br"(?i)bearer\s+[A-Za-z0-9_\-\.]{20,}"),
}

KEYWORD_FIELD = re.compile(
    br"(?i)(?:^|[\s\"'=])(?:cursor|workos|bearer|api[-_]?key)(?:$|[\s\"'=])"
)
TOKEN_FIELD = re.compile(
    br"(?<![A-Za-z0-9])(?=[A-Za-z0-9_\-]*[A-Z])(?=[A-Za-z0-9_\-]*[a-z])[A-Za-z0-9_\-]{40}(?![A-Za-z0-9])"
)


def git(*args, input_bytes=None):
    return subprocess.run(
        ["git", "-C", str(REPO), *args],
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )


def scan_bytes(data: bytes):
    counts = {}
    for name, pattern in PATTERNS.items():
        counts[name] = len(pattern.findall(data))
    keyword_token_lines = 0
    for line in data.splitlines():
        if KEYWORD_FIELD.search(line) and TOKEN_FIELD.search(line):
            keyword_token_lines += 1
    counts["keyword_and_long_field_lines"] = keyword_token_lines
    return counts


def empty_counts():
    return {name: 0 for name in list(PATTERNS) + ["keyword_and_long_field_lines"]}


def add_counts(total, part):
    for key, value in part.items():
        total[key] = total.get(key, 0) + value


def hit_from(counts):
    return {key: value for key, value in counts.items() if value}


def load_paths():
    reachable = defaultdict(list)
    reflog_only_seeds = set()
    proc = git("rev-list", "--objects", "--all")
    if proc.returncode != 0:
        raise SystemExit(f"rev-list --all exit {proc.returncode}")
    for line in proc.stdout.splitlines():
        parts = line.split(b" ", 1)
        sha = parts[0].decode()
        path = parts[1].decode("utf-8", "replace") if len(parts) > 1 else ""
        if path and len(reachable[sha]) < 3:
            reachable[sha].append(path)
    proc_r = git("rev-list", "--objects", "--all", "--reflog")
    reflog_shas = set()
    if proc_r.returncode == 0:
        for line in proc_r.stdout.splitlines():
            sha = line.split(b" ", 1)[0].decode()
            reflog_shas.add(sha)
            if sha not in reachable and len(reflog_only_seeds) < 1:
                pass
    else:
        reflog_shas = None
    return reachable, reflog_shas, proc_r.returncode


def iter_objects():
    proc = subprocess.Popen(
        ["git", "-C", str(REPO), "cat-file", "--batch-all-objects", "--batch"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.stdout is not None
    while True:
        header = proc.stdout.readline()
        if not header:
            break
        parts = header.split()
        if len(parts) < 3:
            continue
        sha, kind, size_b = parts[0].decode(), parts[1].decode(), int(parts[2])
        blob = proc.stdout.read(size_b)
        newline = proc.stdout.read(1)
        if newline != b"\n" and size_b != 0:
            # Keep the stream aligned if a short read happened.
            pass
        yield sha, kind, size_b, blob
    err = proc.stderr.read().decode("utf-8", "replace") if proc.stderr else ""
    code = proc.wait()
    return code, err


def main():
    reachable, reflog_shas, reflog_exit = load_paths()
    totals = empty_counts()
    hits = []
    scanned = 0
    skipped_binary = 0
    skipped_large = 0
    objects = 0
    by_kind = defaultdict(int)
    commit_paths = set()
    commit_list = git("ls-tree", "-r", "--name-only", COMMIT)
    if commit_list.returncode == 0:
        commit_paths = set(commit_list.stdout.decode().splitlines())

    proc = subprocess.Popen(
        ["git", "-C", str(REPO), "cat-file", "--batch-all-objects", "--batch"],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    assert proc.stdout is not None
    while True:
        header = proc.stdout.readline()
        if not header:
            break
        parts = header.split()
        if len(parts) < 3:
            continue
        sha = parts[0].decode()
        kind = parts[1].decode()
        size_b = int(parts[2])
        blob = proc.stdout.read(size_b)
        if proc.stdout.read(1) not in (b"\n", b""):
            pass
        objects += 1
        by_kind[kind] += 1
        if kind != "blob":
            if kind in ("commit", "tag") and size_b <= MAX_BLOB:
                counts = scan_bytes(blob)
                add_counts(totals, counts)
                found = hit_from(counts)
                if found:
                    scope = "reachable" if sha in reachable else "not_in_rev_list_all"
                    if reflog_shas is not None and sha in reflog_shas and sha not in reachable:
                        scope = "reflog_only"
                    hits.append({
                        "object": sha,
                        "kind": kind,
                        "size": size_b,
                        "scope": scope,
                        "paths": reachable.get(sha, [])[:3],
                        "counts": found,
                    })
            continue
        if size_b > MAX_BLOB:
            skipped_large += 1
            continue
        if b"\x00" in blob[:8192]:
            skipped_binary += 1
            continue
        scanned += 1
        counts = scan_bytes(blob)
        add_counts(totals, counts)
        found = hit_from(counts)
        if found:
            paths = reachable.get(sha, [])
            scope = "reachable" if sha in reachable else "not_in_rev_list_all"
            if reflog_shas is not None and sha in reflog_shas and sha not in reachable:
                scope = "reflog_only"
            in_commit = any(path in commit_paths for path in paths)
            hits.append({
                "object": sha,
                "kind": "blob",
                "size": size_b,
                "scope": scope,
                "in_reviewed_commit_tree": in_commit,
                "paths": paths[:3],
                "path_count_capped": len(paths),
                "counts": found,
            })
    cat_err = proc.stderr.read().decode("utf-8", "replace") if proc.stderr else ""
    cat_exit = proc.wait()

    file_hits = []
    file_totals = empty_counts()
    files_scanned = 0
    evidence = REPO / "evidence"
    extra_roots = [evidence]
    tmp_hits_meta = []
    for pattern in ("oo-a077*", "oo-a078*", "*processes-before*"):
        tmp_hits_meta.append(pattern)
    tmp_dirs = []
    tmp = Path("/tmp")
    if tmp.is_dir():
        for child in tmp.iterdir():
            name = child.name
            if name.startswith("oo-a077") or name.startswith("oo-a078") or "processes-before" in name:
                tmp_dirs.append(child)
    scan_roots = extra_roots + tmp_dirs
    for root in scan_roots:
        if not root.exists():
            continue
        paths = [root] if root.is_file() else list(root.rglob("*"))
        for path in paths:
            if not path.is_file():
                continue
            try:
                size = path.stat().st_size
            except OSError:
                continue
            if size > MAX_BLOB:
                continue
            try:
                data = path.read_bytes()
            except OSError:
                continue
            if b"\x00" in data[:8192]:
                continue
            files_scanned += 1
            counts = scan_bytes(data)
            add_counts(file_totals, counts)
            found = hit_from(counts)
            if found:
                file_hits.append({
                    "path": str(path),
                    "size": size,
                    "counts": found,
                })

    # Committed process file, described only when no credential-shaped counts.
    process_rel = "evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/pre-smoke/processes-before.txt"
    show = git("show", f"{COMMIT}:{process_rel}")
    process_report = {"git_show_exit": show.returncode, "size": len(show.stdout)}
    if show.returncode == 0:
        counts = scan_bytes(show.stdout)
        process_report["counts"] = counts
        process_report["line_count"] = show.stdout.count(b"\n")
        process_report["credential_shaped"] = bool(hit_from(counts))
        run = 0
        max_run = 0
        for byte in show.stdout:
            if 48 <= byte <= 57 or 65 <= byte <= 90 or 97 <= byte <= 122:
                run += 1
                if run > max_run:
                    max_run = run
            else:
                run = 0
        process_report["max_alnum_run"] = max_run
        process_report["text_recorded"] = False
    work = REPO / process_rel
    if work.is_file():
        work_bytes = work.read_bytes()
        process_report["worktree_size"] = len(work_bytes)
        process_report["worktree_matches_commit"] = hashlib.sha256(work_bytes).hexdigest() == hashlib.sha256(show.stdout).hexdigest()
        # The equality digest is of the whole evidence file, not of an extracted credential.
        process_report["worktree_sha256"] = hashlib.sha256(work_bytes).hexdigest()
        process_report["commit_file_sha256"] = hashlib.sha256(show.stdout).hexdigest()

    count_objects = git("count-objects", "-v")
    fsck = git("fsck", "--connectivity-only", "--no-progress")
    fsck_lines = []
    dangling = defaultdict(int)
    for line in fsck.stdout.splitlines():
        text = line.decode("utf-8", "replace")
        if text.startswith("dangling "):
            kind = text.split()[1] if len(text.split()) >= 2 else "unknown"
            dangling[kind] += 1
        elif text.startswith("unreachable "):
            kind = text.split()[1] if len(text.split()) >= 2 else "unknown"
            dangling["unreachable_" + kind] += 1
        else:
            if len(fsck_lines) < 30 and "error" in text.lower():
                fsck_lines.append(text[:200])

    contains = git("branch", "-r", "--contains", COMMIT)
    remote_branches = []
    if contains.returncode == 0:
        remote_branches = [line.strip().decode() for line in contains.stdout.splitlines() if line.strip()]

    report = {
        "commit": COMMIT,
        "git_objects": objects,
        "git_objects_by_kind": dict(by_kind),
        "text_blobs_scanned": scanned,
        "binary_blobs_skipped": skipped_binary,
        "large_blobs_skipped": skipped_large,
        "cat_file_exit": cat_exit,
        "cat_file_stderr_bytes": len(cat_err.encode()),
        "pattern_totals_git": totals,
        "git_hit_count": len(hits),
        "git_hits": hits[:200],
        "git_hits_truncated": len(hits) > 200,
        "evidence_and_tmp_files_scanned": files_scanned,
        "pattern_totals_files": file_totals,
        "file_hit_count": len(file_hits),
        "file_hits": file_hits[:200],
        "processes_before": process_report,
        "count_objects_exit": count_objects.returncode,
        "count_objects": count_objects.stdout.decode("utf-8", "replace"),
        "fsck_connectivity_exit": fsck.returncode,
        "fsck_dangling_or_unreachable_counts": dict(dangling),
        "fsck_other_error_lines": fsck_lines,
        "reflog_rev_list_exit": reflog_exit,
        "remote_tracking_branches_containing_commit": remote_branches,
        "not_scanned": [
            "credential stores",
            "shell history",
            "Cursor application data",
            "agent session transcripts",
            "foreign container logs",
            "backups",
            "remotes over the network",
        ],
    }
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps({
        "wrote": str(OUT),
        "git_hit_count": len(hits),
        "file_hit_count": len(file_hits),
        "text_blobs_scanned": scanned,
        "objects": objects,
        "process_credential_shaped": process_report.get("credential_shaped"),
        "fsck_connectivity_exit": fsck.returncode,
        "dangling": dict(dangling),
    }))


if __name__ == "__main__":
    main()
