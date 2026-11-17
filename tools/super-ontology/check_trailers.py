#!/usr/bin/env python3
"""Commit trailer check (Super Ontology 50 sec.10 rule 1, card B-S1-06).

For every non-merge commit in <base>..<head> that carries a `Plan: SO-...` line:
  * exactly one `Plan:` line, value matching PLAN_RE
  * no Co-Authored-By / agent attribution
  * author and committer are the owner identity
  * commits adding a file under migrations/ carry `Migration:`
  * commits touching contracts/docs-api/ carry `Contract-Version:`
Commits without a `Plan: SO-` line (pre-existing history) are ignored.
Exit 0 ok, 1 violations, 2 usage/git error.
"""
import argparse
import re
import subprocess
import sys

PLAN_RE = re.compile(r"^SO-(B|F|V)-S\d+-\d{2}$|^SO-SPEC$|^SO-SIGN-L\d$")
OWNER_NAME = "Redux0223"
OWNER_EMAIL = "171701490+Redux0223@users.noreply.github.com"
MIGRATION_PREFIX = "migrations/"
CONTRACT_PREFIX = "contracts/docs-api/"
AGENT_RES = [
    re.compile(r"^\s*co-authored-by\s*:", re.I | re.M),
    re.compile(r"generated with .*(claude|codex|copilot|gemini|cursor)", re.I),
    re.compile(r"noreply@anthropic\.com", re.I),
    re.compile(r"^\s*(signed-off-by|assisted-by|generated-by)\s*:.*\b(claude|codex|copilot|gemini|gpt|ai)\b", re.I | re.M),
    re.compile(r"🤖"),
]
SEP = "\x1e"
FIELD = "\x1f"


def git(repo, *args):
    p = subprocess.run(["git", "-C", repo, *args], capture_output=True, text=True)
    if p.returncode != 0:
        raise RuntimeError(f"git {' '.join(args)} failed: {p.stderr.strip()}")
    return p.stdout


def trailer_values(body, key):
    return [m.group(1).strip() for m in re.finditer(rf"^{re.escape(key)}:[ \t]*(.*)$", body, re.M)]


def commits(repo, rev_range):
    fmt = FIELD.join(["%H", "%an", "%ae", "%cn", "%ce", "%B"]) + SEP
    out = git(repo, "log", "--no-merges", "--reverse", f"--format={fmt}", rev_range)
    for rec in out.split(SEP):
        rec = rec.strip("\n")
        if not rec:
            continue
        sha, an, ae, cn, ce, body = rec.split(FIELD, 5)
        yield sha, an, ae, cn, ce, body


def changed_files(repo, sha):
    out = git(repo, "diff-tree", "--no-commit-id", "--name-status", "-r", "--root", "-m", "--first-parent", sha)
    res = []
    for line in out.splitlines():
        parts = line.split("\t")
        res.append((parts[0][0], parts[-1]))
    return res


def check(repo, rev_range, owner_name=OWNER_NAME, owner_email=OWNER_EMAIL):
    """Return list of (short_sha, message) violations."""
    bad = []
    for sha, an, ae, cn, ce, body in commits(repo, rev_range):
        plans = trailer_values(body, "Plan")
        if not any(p.startswith("SO-") for p in plans):
            continue
        s = sha[:9]
        if len(plans) != 1:
            bad.append((s, f"expected exactly one Plan: trailer, found {len(plans)}"))
        for p in plans:
            if not PLAN_RE.match(p):
                bad.append((s, f"Plan value {p!r} does not match the allowed pattern"))
        for rx in AGENT_RES:
            if rx.search(body):
                bad.append((s, "Co-Authored-By or agent attribution present"))
                break
        for role, n, e in (("author", an, ae), ("committer", cn, ce)):
            if n != owner_name or e != owner_email:
                bad.append((s, f"{role} is not the owner identity"))
        files = changed_files(repo, sha)
        if any(st == "A" and (f.startswith(MIGRATION_PREFIX) or "/" + MIGRATION_PREFIX in f) for st, f in files):
            if not trailer_values(body, "Migration"):
                bad.append((s, "adds a migration but has no Migration: trailer"))
        if any(f.startswith(CONTRACT_PREFIX) for _, f in files):
            if not trailer_values(body, "Contract-Version"):
                bad.append((s, "changes contracts/docs-api/ but has no Contract-Version: trailer"))
    return bad


def main(argv=None):
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--repo", default=".")
    ap.add_argument("--base", required=True, help="base ref; checks <base>..<head>")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--owner-name", default=OWNER_NAME)
    ap.add_argument("--owner-email", default=OWNER_EMAIL)
    a = ap.parse_args(argv)
    try:
        bad = check(a.repo, f"{a.base}..{a.head}", a.owner_name, a.owner_email)
    except RuntimeError as e:
        print(e, file=sys.stderr)
        return 2
    for s, m in bad:
        print(f"{s}: {m}")
    print(f"trailer violations: {len(bad)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
