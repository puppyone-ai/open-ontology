#!/usr/bin/env python3
"""Commit one registered worker's paths through a private Git index."""
import argparse
import fcntl
import json
import os
from pathlib import Path, PurePosixPath
import subprocess
import tempfile


def git(*args, env=None):
    return subprocess.check_output(["git", *args], env=env)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--task", required=True)
    parser.add_argument("--message", required=True)
    parser.add_argument("--dry-run", action="store_true")
    args = parser.parse_args()
    env = dict(os.environ)
    env.pop("GIT_INDEX_FILE", None)
    root = Path(git("rev-parse", "--show-toplevel", env=env).decode().strip())
    os.chdir(root)
    with open("/tmp/oo-dl01-wave1-git.lock", "a") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        manifest = json.loads(git("show", "HEAD:execution/batches/dl01-wave1-20261009/dispatch.json", env=env))
        matches = [t for t in manifest["tasks"] if t["task_id"] == args.task]
        if len(matches) != 1:
            raise SystemExit("task must have exactly one committed ownership record")
        paths = matches[0]["write_paths"]
        for path in paths:
            parsed = PurePosixPath(path)
            if parsed.is_absolute() or not parsed.parts or any(p in (".", "..", ".git") for p in parsed.parts) or path in (".", ""):
                raise SystemExit("invalid ownership prefix")
        def owned(path):
            return any(path == prefix or path.startswith(prefix.rstrip("/") + "/") for prefix in paths)
        def foreign_index():
            entries = git("ls-files", "--stage", "-z", env=env).split(b"\0")
            return sorted(entry for entry in entries if entry and not owned(entry.split(b"\t", 1)[1].decode("utf-8", "surrogateescape")))
        before = foreign_index()
        with tempfile.TemporaryDirectory(prefix="oo-owned-index-") as tmp:
            private = dict(env, GIT_INDEX_FILE=str(Path(tmp) / "index"))
            subprocess.run(["git", "read-tree", "HEAD"], env=private, check=True)
            # Ignored raw evidence stays local under the current sanitized-snapshot policy.
            present = [p for p in paths
                       if (Path(p).exists() or git("ls-files", "--", p, env=private).strip())
                       and subprocess.run(["git", "check-ignore", "-q", "--", p], env=private).returncode != 0]
            if present:
                subprocess.run(["git", "add", "-A", "--", *present], env=private, check=True)
            changed = [p.decode("utf-8", "surrogateescape") for p in git("diff", "--cached", "--name-only", "-z", env=private).split(b"\0") if p]
            if not all(owned(p) for p in changed):
                raise SystemExit("private index contains a foreign path")
            subprocess.run(["git", "diff", "--cached", "--check"], env=private, check=True)
            if args.dry_run or not changed:
                print(json.dumps({"task": args.task, "dry_run": args.dry_run, "changed": changed, "no_changes": not changed, "foreign_index_preserved": before == foreign_index()}))
                return
            subprocess.run(["git", "commit", "-m", args.message, "--only", "--", *changed], env=private, check=True)
            commit = git("rev-parse", "HEAD", env=private).decode().strip()
            committed = [p.decode("utf-8", "surrogateescape") for p in git("diff-tree", "--no-commit-id", "--name-only", "-r", "-z", commit, env=private).split(b"\0") if p]
            if not all(owned(p) for p in committed):
                raise SystemExit("committed ownership invariant failed; preserve commit for coordinator")
            subprocess.run(["git", "reset", "--quiet", "HEAD", "--", *changed], env=env, check=True)
            if before != foreign_index():
                raise SystemExit("foreign staged entries changed; preserve commit and notify coordinator")
            print(json.dumps({"task": args.task, "commit": commit, "changed": committed, "foreign_index_preserved": True}))


if __name__ == "__main__":
    main()
