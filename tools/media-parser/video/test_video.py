import importlib.util
import json
import os
import shutil
import sys
from pathlib import Path

import pytest

ROOT = Path(__file__).resolve().parents[1]
for p in (ROOT, ROOT.parent / "doc-parser"):
    sys.path.insert(0, str(p))

from video import frames as vf  # noqa: E402
from video import keyframes as kf  # noqa: E402
from video import synth  # noqa: E402
from video import timeline as tl  # noqa: E402


def test_one_key_frame_per_slide_despite_a_live_clock_and_noise():
    fr = synth.slide_frames()
    kept = kf.dedupe([(t, img) for t, img, _ in fr])
    assert len(kept) == len(synth.SLIDES) == 8
    # the key frame is the first frame of each slide
    starts, t = [], 0
    for d in [6, 8, 5, 7, 6, 5, 6, 4]:
        starts.append(t * 1000)
        t += d
    assert [k[0] for k in kept] == starts
    spans = kf.scenes(kept, t * 1000)
    assert spans[0] == (0, 6000) and spans[-1][1] == t * 1000


def test_pyscenedetect_hash_detector_finds_every_page_turn_when_installed():
    pytest.importorskip("scenedetect")
    fr = [(t, img) for t, img, _ in synth.slide_frames()]
    cuts = kf.scenedetect_cuts(fr)
    assert cuts == {6, 14, 19, 26, 32, 37, 43}
    # the union with the numpy signals keeps exactly one key frame per slide
    assert len(kf.dedupe(fr, use_scenedetect=True)) == 8 and len(kf.dedupe(fr, use_scenedetect=False)) == 8


def test_a_page_turn_is_far_above_the_hash_threshold_and_a_clock_tick_far_below():
    a = synth.slide_image(synth.SLIDES[1], tick=1)
    b = synth.slide_image(synth.SLIDES[1], tick=2, seed=9)
    c = synth.slide_image(synth.SLIDES[2], tick=3)
    assert kf.hamming(kf.dhash_bits(a), kf.dhash_bits(b)) <= kf.HASH_DISTANCE
    assert kf.hamming(kf.dhash_bits(a), kf.dhash_bits(c)) > kf.HASH_DISTANCE


def test_keyframe_budget_samples_long_scenes_first_and_warns():
    spans = [(i * 1000, i * 1000 + (10 if i % 2 else 1) * 1000) for i in range(20)]
    idx, warn = kf.cap_keyframes(spans, duration_ms=3_600_000, per_hour=5, min_budget=1)
    assert warn == "DOCS_PARSE_KEYFRAME_SAMPLED" and len(idx) == 5 and idx == sorted(idx)
    assert all(i % 2 == 1 for i in idx), "the longest scenes win"
    idx2, warn2 = kf.cap_keyframes(spans, duration_ms=3_600_000, per_hour=600)
    assert warn2 is None and idx2 == list(range(20))
    # the budget scales with the video length: 30 minutes allow 300
    assert len(kf.cap_keyframes([(i, i + 1) for i in range(400)], 1_800_000)[0]) == 300


def test_timeline_orders_by_pts_and_applies_the_start_offset():
    kept = [(0, None, 0), (10_000, None, 1)]
    spans = [(0, 10_000), (10_000, 20_000)]
    lines = {0: [{"text": "标题", "score": 0.9, "bbox_px": [10, 10, 100, 40]}], 1: [{"text": "标题", "score": 0.8, "bbox_px": [1, 2, 3, 4]}]}
    utts = [{"speaker": "S1", "t_start_ms": 500, "t_end_ms": 4000, "text": "大家好"},
            {"speaker": "S2", "t_start_ms": 10_000, "t_end_ms": 12_000, "text": "进入下一页"}]
    items = tl.build_timeline(kept, spans, lines, utts, offset_ms=0)
    assert [(i["kind"], i.get("t_start_ms", i.get("t_ms"))) for i in items] == [
        ("scene", 0), ("frame_text", 0), ("utterance", 500), ("scene", 10_000), ("frame_text", 10_000), ("utterance", 10_000)]
    shifted = tl.build_timeline(kept, spans, lines, [], offset_ms=250)
    assert shifted[0]["t_start_ms"] == 250 and shifted[1]["t_ms"] == 250
    assert json.dumps(items) == json.dumps(tl.build_timeline(kept, spans, lines, utts, 0)), "deterministic"
    assert tl.merge_scene_text([[{"text": "A b"}], [{"text": "Ab"}, {"text": "C"}]]) == "A b\nC"
    assert tl.frame_record(0, [{"text": " ", "bbox_px": [0, 0, 1, 1]}]) is None


def test_analyse_runs_the_steps_with_a_fake_ocr():
    fr = [(t, img) for t, img, _ in synth.slide_frames()]
    calls = []

    def fake_ocr(img):
        calls.append(1)
        return [{"text": f"S{len(calls)}", "score": 0.99, "bbox_px": [0, 0, 10, 10]}]

    out = vf.analyse(fr, 47_000, fake_ocr, per_hour=600)
    assert out["key_frames"] == 8 and out["ocr_frames"] == 8 and len(calls) == 8 and out["warnings"] == []
    assert [i["kind"] for i in out["items"][:2]] == ["scene", "frame_text"]


def _ocr_ready():
    models = os.environ.get("DOCLING_ARTIFACTS_PATH", "/opt/models")
    return (importlib.util.find_spec("rapidocr") is not None and synth.has_cjk_font()
            and (Path(models) / "rapidocr-ppocrv5" / "det.onnx").is_file())


@pytest.mark.skipif(not _ocr_ready(), reason="needs rapidocr, its pinned models (DOCLING_ARTIFACTS_PATH) and a CJK font")
def test_frame_text_recall_is_at_least_0_9_with_rapidocr():
    from doc_parser import pictures

    from video.keyframes import dedupe

    fr = synth.slide_frames()
    kept = dedupe([(t, img) for t, img, _ in fr])
    got = 0
    total = 0
    from collections import Counter

    for (t, img, scene), slide in zip(kept, range(len(kept))):
        text = "".join(l["text"] for l in pictures.read_lines(img))
        truth = Counter("".join(synth.truth_text(slide).split()))
        have = Counter("".join(text.split()))
        got += sum((truth & have).values())
        total += sum(truth.values())
    assert got / total >= 0.9, got / total
