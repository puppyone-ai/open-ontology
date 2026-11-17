import os
import subprocess
from pathlib import Path

import check_trailers as ct

OWNER = {"GIT_AUTHOR_NAME": ct.OWNER_NAME, "GIT_AUTHOR_EMAIL": ct.OWNER_EMAIL,
         "GIT_COMMITTER_NAME": ct.OWNER_NAME, "GIT_COMMITTER_EMAIL": ct.OWNER_EMAIL}


def g(repo, *a, env=None):
    e = {**os.environ, **OWNER, **(env or {})}
    return subprocess.run(["git", "-C", str(repo), *a], check=True, capture_output=True, text=True, env=e).stdout


def commit(repo, msg, files=None, env=None):
    for name, content in (files or {"f.txt": msg}).items():
        p = Path(repo) / name
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content)
    g(repo, "add", "-A")
    g(repo, "commit", "-q", "-m", msg, env=env)


def repo(tmp_path):
    r = tmp_path / "r"
    r.mkdir()
    g(r, "init", "-q", "-b", "main")
    commit(r, "old history commit")
    g(r, "tag", "base")
    return r


def run(r):
    return ct.check(str(r), "base..HEAD")


def test_trailers_good_commit_passes(tmp_path):
    r = repo(tmp_path)
    commit(r, "Do a thing\n\nwhy\n\nPlan: SO-B-S1-06\nDecision: D1")
    commit(r, "Spec\n\nPlan: SO-SPEC")
    commit(r, "Sign\n\nPlan: SO-SIGN-L1")
    assert run(r) == []


def test_trailers_old_history_ignored(tmp_path):
    r = repo(tmp_path)
    commit(r, "No plan here", env={"GIT_AUTHOR_NAME": "Someone", "GIT_AUTHOR_EMAIL": "x@y.z"})
    commit(r, "Other plan style\n\nPlan: legacy-1")
    assert run(r) == []


def test_trailers_two_plans_fail(tmp_path):
    r = repo(tmp_path)
    commit(r, "x\n\nPlan: SO-B-S1-01\nPlan: SO-B-S1-02")
    assert any("exactly one" in m for _, m in run(r))


def test_trailers_bad_plan_value_fails(tmp_path):
    r = repo(tmp_path)
    for i, v in enumerate(["SO-X-S1-01", "SO-B-S1-1", "SO-SIGN-L", "SO-spec"]):
        commit(r, f"x{i}\n\nPlan: {v}")
    assert len(run(r)) == 4


def test_trailers_coauthor_and_agent_fail(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01\nCo-Authored-By: Someone <a@b.c>")
    commit(r, "b\n\nPlan: SO-B-S1-02\nco-authored-by: Bot <noreply@anthropic.com>")
    commit(r, "c\n\nPlan: SO-B-S1-03\n\n🤖 Generated with [Claude Code](https://claude.com/claude-code)")
    assert [m for _, m in run(r)].count("Co-Authored-By or agent attribution present") == 3


def test_trailers_identity_fail(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01", env={"GIT_AUTHOR_EMAIL": "other@example.com"})
    commit(r, "b\n\nPlan: SO-B-S1-02", env={"GIT_COMMITTER_NAME": "Other"})
    msgs = [m for _, m in run(r)]
    assert "author is not the owner identity" in msgs
    assert "committer is not the owner identity" in msgs


def test_trailers_migration_requires_trailer(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01", files={"migrations/0137_x.sql": "select 1;"})
    assert any("Migration:" in m for _, m in run(r))
    commit(r, "b\n\nPlan: SO-B-S1-02\nMigration: 0138", files={"migrations/0138_y.sql": "select 1;"})
    assert len(run(r)) == 1
    # modifying (not adding) an existing migration does not require the trailer
    commit(r, "c\n\nPlan: SO-B-S1-03", files={"migrations/0138_y.sql": "select 2;"})
    assert len(run(r)) == 1


def test_trailers_contract_requires_trailer(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01", files={"contracts/docs-api/a.json": "{}"})
    assert any("Contract-Version:" in m for _, m in run(r))
    commit(r, "b\n\nPlan: SO-B-S1-02\nContract-Version: docs-api/1.0", files={"contracts/docs-api/b.json": "{}"})
    assert len(run(r)) == 1


def test_trailers_merge_commits_skipped(tmp_path):
    r = repo(tmp_path)
    g(r, "checkout", "-q", "-b", "side")
    commit(r, "s\n\nPlan: SO-B-S1-01", files={"s.txt": "s"})
    g(r, "checkout", "-q", "main")
    commit(r, "m\n\nPlan: SO-B-S1-02", files={"m.txt": "m"})
    g(r, "merge", "-q", "--no-ff", "-m", "Merge side without plan", "side")
    assert run(r) == []


def test_trailers_cli_exit_codes(tmp_path):
    r = repo(tmp_path)
    commit(r, "a\n\nPlan: SO-B-S1-01")
    assert ct.main(["--repo", str(r), "--base", "base"]) == 0
    commit(r, "b\n\nPlan: SO-B-S1-02\nPlan: SO-B-S1-03")
    assert ct.main(["--repo", str(r), "--base", "base"]) == 1
    assert ct.main(["--repo", str(r), "--base", "nonexistent"]) == 2
