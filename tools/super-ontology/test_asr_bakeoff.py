import json
import subprocess
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import asr_bakeoff as b  # noqa: E402

ROOT = Path(__file__).resolve().parents[2]


def test_cer_ignores_punctuation_and_counts_character_edits():
    assert b.cer("你好，世界！", "你好世界") == 0.0
    assert b.cer("星图看板", "星图看版") == 0.25
    assert b.cer("abc", "") == 1.0
    assert b.cer("A b", "ab") == 0.0, "case and spaces do not count"


def test_der_maps_speakers_optimally_and_counts_all_three_error_kinds():
    ref = [("A", 0, 1000), ("B", 1000, 2000), ("A", 2000, 3000)]
    assert b.der(ref, [("x", 0, 1000), ("y", 1000, 2000), ("x", 2000, 3000)], 3000) == 0.0
    # swapped labels are still perfect after the optimal mapping
    assert b.der(ref, [("y", 0, 1000), ("x", 1000, 2000), ("y", 2000, 3000)], 3000) == 0.0
    # one second given to the wrong speaker: confusion 1000 of 3000 ms
    assert b.der(ref, [("x", 0, 1000), ("x", 1000, 2000), ("x", 2000, 3000)], 3000) == pytest.approx(1 / 3, abs=0.01)
    # missed speech and false alarm
    assert b.der([("A", 0, 1000)], [("x", 500, 1500)], 2000) == pytest.approx(1.0, abs=0.02)
    assert b.der([], [("x", 0, 10)], 100) == 0.0


def test_script_is_synthetic_with_three_speakers_and_no_overlap_requirement():
    script = b.load_script(ROOT / "testing/super-ontology/golden/media")
    assert set(script["speakers"]) == {"A", "B", "C"}
    assert {t[0] for t in script["turns"]} == {"A", "B", "C"} and len(script["turns"]) >= 12
    assert "合成" in script["note"] or "Synthetic" in script["note"]


def test_license_table_marks_forbidden_components():
    assert "GPL" in b.LICENSES["qwen3-asr-0.6b"][1] or "qwen-asr" in b.LICENSES["qwen3-asr-0.6b"][1]
    assert "gated" in b.LICENSES["pyannote-community-1"][1] and "excluded" in b.LICENSES["sensevoice-small"][1]


def test_cli_self_check_runs_with_the_standard_library_only():
    p = subprocess.run([sys.executable, "-B", str(Path(b.__file__)), "--metrics-only"], capture_output=True, text=True)
    assert p.returncode == 0 and "metrics ok" in p.stdout


def test_report_lists_unrun_candidates_with_the_reason():
    rows = [{"name": "x", "kind": "asr", "lic_key": "paraformer-zh", "status": "not run: no network"}]
    text = b.render_report(rows, 120.0, "n")
    assert "not run: no network" in text and "| x | asr |" in text
