import subprocess
import sys
from pathlib import Path

SCRIPT = Path(__file__).with_name("check_private_terms.py")
TERM = "zzqx-synthetic-term"


def run(tmp_path, env_file, *paths):
    env = {"PATH": "/usr/bin:/bin"}
    if env_file is not None:
        env["OO_PRIVATE_TERMS_FILE"] = str(env_file)
    return subprocess.run([sys.executable, "-B", str(SCRIPT), *map(str, paths)],
                          capture_output=True, text=True, env=env)


def make(tmp_path):
    terms = tmp_path / "terms.txt"
    terms.write_text(f"# comment\n\n{TERM}\n")
    d = tmp_path / "data"
    d.mkdir()
    (d / "clean.md").write_text("nothing here\n")
    return terms, d


def test_private_clean_tree_passes(tmp_path):
    terms, d = make(tmp_path)
    r = run(tmp_path, terms, d)
    assert r.returncode == 0
    assert "hits: 0" in r.stdout


def test_private_hit_reports_location_not_term(tmp_path):
    terms, d = make(tmp_path)
    (d / "bad.md").write_text(f"line1\nsome {TERM.upper()} here\n")
    r = run(tmp_path, terms, d)
    assert r.returncode == 1
    assert "bad.md:2" in r.stdout
    assert "hits: 1" in r.stdout
    assert TERM not in r.stdout.lower() and TERM not in r.stderr.lower()


def test_private_binary_hit(tmp_path):
    terms, d = make(tmp_path)
    (d / "b.bin").write_bytes(b"\0\1" + TERM.encode() + b"\0")
    r = run(tmp_path, terms, d)
    assert r.returncode == 1 and "b.bin:0" in r.stdout


def test_private_missing_terms_file_is_error(tmp_path):
    _, d = make(tmp_path)
    r = run(tmp_path, tmp_path / "nope.txt", d)
    assert r.returncode == 2
    assert "nope" not in r.stdout


def test_private_skips_git_dir(tmp_path):
    terms, d = make(tmp_path)
    (d / ".git").mkdir()
    (d / ".git" / "x").write_text(TERM)
    assert run(tmp_path, terms, d).returncode == 0
