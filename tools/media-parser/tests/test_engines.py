import os
from pathlib import Path

import numpy as np
import pytest

from asr import paraformer_onnx as pf

MODELS = Path(os.environ.get("OO_MEDIA_MODELS", "/opt/models"))
HAVE = (MODELS / "paraformer-zh" / "model_quant.onnx").is_file()


def test_lfr_stacks_seven_frames_every_sixth_with_left_padding():
    feat = np.arange(20 * 3, dtype=np.float32).reshape(20, 3)
    out = pf.lfr(feat, 7, 6)
    assert out.shape == (4, 21)
    # the first output frame is 3 copies of frame 0 (left context), then frames 0..3
    assert (out[0, :9] == np.tile(feat[0], 3)).all() and (out[0, 9:12] == feat[0]).all() and (out[0, 12:15] == feat[1]).all()
    assert pf.lfr(feat[:1], 7, 6).shape == (1, 21)


def test_cmvn_file_is_parsed_and_applied_as_shift_then_scale(tmp_path):
    f = tmp_path / "am.mvn"
    f.write_text("<Nnet>\n<AddShift> 3 3\n<LearnRateCoef> 0 [ 1 2 3 ]\n<Rescale> 3 3\n<LearnRateCoef> 0 [ 2 2 2 ]\n</Nnet>\n")
    means, scale = pf.load_cmvn(f)
    assert means.tolist() == [1, 2, 3] and scale.tolist() == [2, 2, 2]
    with pytest.raises(ValueError):
        (tmp_path / "bad.mvn").write_text("<Nnet>\n</Nnet>\n")
        pf.load_cmvn(tmp_path / "bad.mvn")


def test_word_pieces_merge_and_chinese_stays_per_character():
    assert pf.merge_pieces(["各", "ab@@", "cd", "位"]) == ["各", "abcd", "位"]
    assert pf.merge_pieces(["x@@"]) == ["x"]


def test_tokens_are_placed_on_voiced_time_only():
    sr = pf.SR
    x = np.zeros(sr * 4, dtype=np.float32)
    x[: sr] = 0.3        # voiced 0..1 s
    x[3 * sr:] = 0.3     # voiced 3..4 s, silence in between
    spans = pf.place_tokens(4, x)
    assert spans[0][0] < 200 and spans[1][1] <= 1100, "first half of the tokens in the first voiced second"
    assert spans[2][0] >= 2900 and spans[3][1] <= 4100, "second half after the silence"
    assert all(a <= b for a, b in spans)
    assert pf.place_tokens(0, x) == []
    flat = pf.place_tokens(2, np.zeros(sr, dtype=np.float32))
    assert flat == [(0, 500), (500, 1000)]


def test_fbank_shapes_and_the_speaker_feature_is_mean_normalised():
    from asr import embed

    x = (0.2 * np.sin(2 * np.pi * 300 * np.arange(16000) / 16000)).astype(np.float32)
    f = embed.fbank80(x)
    assert f.shape == (98, 80) and abs(float(f.mean(axis=0).max())) < 1e-3
    assert embed.fbank80(np.zeros(100, dtype=np.float32)).shape == (0, 80)
    assert pf.fbank(x).shape == (98, 80)


@pytest.mark.skipif(not HAVE, reason="needs the media models (OO_MEDIA_MODELS)")
def test_paraformer_and_campplus_and_vad_run_on_a_tone_without_crashing():
    from asr.embed import CampPlusOnnx
    from asr.vad import SileroVad

    sr = 16000
    x = (0.3 * np.sin(2 * np.pi * 220 * np.arange(sr * 3) / sr)).astype(np.float32)
    v = SileroVad(MODELS / "silero-vad" / "silero_vad.onnx")
    probs = v.probs(x)
    assert len(probs) == len(x) // 512 and all(0.0 <= p <= 1.0 for p in probs)
    e = CampPlusOnnx(MODELS / "campplus" / "campplus.onnx")
    a, b = e.embed(x[:sr * 2]), e.embed(x[sr:])
    assert a.shape == (192,) and float(a @ b / np.linalg.norm(a) / np.linalg.norm(b)) > 0.9, "the same tone is the same voice"
    eng = pf.ParaformerOnnx(MODELS / "paraformer-zh")
    assert isinstance(eng.transcribe(x), list)
