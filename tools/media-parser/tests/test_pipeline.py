import json

import numpy as np
import pytest

from asr import chunks as ck
from asr import diarize as dz
from asr.cache import ChunkCache, chunk_key
from asr.pipeline import Pipeline, build_utterances, join_text, split_long


def make(tone, tmp_path, name="job", **kw):
    return Pipeline(tone["vad"], tone["engine"], tone["embedder"], tmp_path / name / "chunks", tmp_path / name / "emb", **kw)


def test_speech_segments_pad_merge_and_drop_short():
    probs = [0] * 5 + [1] * 10 + [0] * 3 + [1] * 10 + [0] * 30 + [1] * 2 + [0] * 5   # frame = 32 ms
    segs = ck.speech_segments(probs, total_ms=len(probs) * 32)
    # the 3-frame gap (96 ms) is bridged, the 2-frame blip (64 ms) is dropped
    assert len(segs) == 1 and segs[0][0] == 160 - 60 and segs[0][1] == 28 * 32 + 60


def test_chunks_are_disjoint_within_30_s_and_hard_cut_at_the_weakest_frame():
    probs = [1.0] * 2000
    probs[900] = 0.1  # a dip inside the search window before the 30 s limit (30000 ms = frame 937)
    chunks = ck.make_chunks([(0, 64_000)], probs)
    assert chunks[0] == (0, 900 * 32) and all(b - a <= 30_000 for a, b in chunks)
    assert all(chunks[i][1] == chunks[i + 1][0] for i in range(len(chunks) - 1)), "no gap and no overlap in a run"
    joined = ck.make_chunks([(0, 12_000), (13_000, 24_000), (25_000, 40_000)])
    assert joined == [(0, 24_000), (25_000, 40_000)]


def test_alignment_windows_are_at_most_300_seconds():
    chunks = [(i * 30_000, (i + 1) * 30_000) for i in range(25)]
    wins = ck.alignment_windows(chunks)
    assert all(chunks[w[-1]][1] - chunks[w[0]][0] <= 300_000 for w in wins)
    assert sum(len(w) for w in wins) == 25 and wins[0] == list(range(10))


def test_each_word_belongs_to_exactly_one_chunk_by_its_midpoint():
    words = [{"start_ms": 29_000, "end_ms": 31_500}, {"start_ms": 29_900, "end_ms": 30_100}, {"start_ms": 30_000, "end_ms": 32_000}]
    first = ck.own_words(words, 0, 30_000)
    second = ck.own_words(words, 30_000, 60_000)
    assert len(first) + len(second) == 3 and not any(w in second for w in first)


def test_cache_key_changes_with_pcm_interval_revision_and_params(tmp_path):
    k = chunk_key(b"abc", 0, 10, "m1", {"a": 1})
    assert k == chunk_key(b"abc", 0, 10, "m1", {"a": 1})
    assert len({k, chunk_key(b"abd", 0, 10, "m1", {"a": 1}), chunk_key(b"abc", 1, 10, "m1", {"a": 1}),
                chunk_key(b"abc", 0, 10, "m2", {"a": 1}), chunk_key(b"abc", 0, 10, "m1", {"a": 2})}) == 5
    c = ChunkCache(tmp_path / "c")
    assert c.get(k) is None
    c.put(k, {"x": 1})
    assert c.get(k) == {"x": 1} and c.keys() == [k]
    assert not list((tmp_path / "c").glob(".*tmp")), "tmp-and-rename leaves no partial file"


def test_three_speakers_are_recovered_in_order_of_first_speech(tone, tmp_path):
    wav = tmp_path / "m.wav"
    truth = tone["synth"]([("B", 4000), ("A", 5000), ("C", 4000), ("B", 3000), ("A", 3000)], wav)
    out = make(tone, tmp_path).run(wav, offset_ms=0)
    assert out["speakers"] == ["S1", "S2", "S3"]
    # B spoke first, so B is S1; A is S2; C is S3
    order = [u["speaker"] for u in out["utterances"]]
    assert order == ["S1", "S2", "S3", "S1", "S2"]
    for u, (who, a, b) in zip(out["utterances"], truth):
        assert abs(u["t_start_ms"] - a) <= 1700 and abs(u["t_end_ms"] - b) <= 1700
    assert out["utterances"][0]["word_start"] == 0 and out["word_count"] > 0


def test_times_move_to_the_source_axis_by_the_start_offset(tone, tmp_path):
    wav = tmp_path / "m.wav"
    tone["synth"]([("A", 3000)], wav)
    plain = make(tone, tmp_path, "a").run(wav, 0)["utterances"][0]
    shifted = make(tone, tmp_path, "b").run(wav, 2500)["utterances"][0]
    assert shifted["t_start_ms"] == plain["t_start_ms"] + 2500 and shifted["t_end_ms"] == plain["t_end_ms"] + 2500


def test_resume_does_not_redo_finished_chunks(tone, tmp_path):
    wav = tmp_path / "m.wav"
    tone["synth"]([("A", 20_000), ("B", 20_000), ("A", 20_000), ("B", 20_000)], wav, gap_ms=1500)

    def die_after_two(done_ms, total_ms, i, n, reused):
        if i == 1:
            raise KeyboardInterrupt("kill")

    first = make(tone, tmp_path, on_chunk=die_after_two)
    with pytest.raises(KeyboardInterrupt):
        first.run(wav)
    assert tone["engine"].calls == 2
    emb_before = (tmp_path / "job" / "emb").is_dir()
    assert emb_before, "features of finished chunks stay while the job is unfinished"
    second = make(tone, tmp_path)
    out = second.run(wav)
    assert second.stats["reused"] == 2 and second.stats["computed"] == out["chunks"] - 2
    assert tone["engine"].calls == out["chunks"], "finished chunks were not transcribed again"
    assert not (tmp_path / "job" / "emb").exists(), "speaker features are gone when the file is done"


def test_same_input_gives_the_same_bytes_with_or_without_a_resume(tone, tmp_path):
    wav = tmp_path / "m.wav"
    tone["synth"]([("A", 7000), ("B", 7000), ("C", 7000)], wav)
    a = make(tone, tmp_path, "x").run(wav)
    b = make(tone, tmp_path, "y").run(wav)
    ser = lambda o: json.dumps(o, sort_keys=True, ensure_ascii=False, separators=(",", ":"))
    assert ser(a) == ser(b)
    c = make(tone, tmp_path, "x").run(wav)  # everything from the cache
    assert ser(a) == ser(c)


def test_embeddings_are_files_only_inside_the_job_and_cluster_is_deterministic(tmp_path):
    store = dz.EmbeddingStore(tmp_path / "emb")
    store.put("k1", np.ones((3, 4)))
    assert store.files() == ["k1.f32"] and store.get("k1").shape == (3, 4)
    store.delete()
    assert store.files() == [] and not (tmp_path / "emb").exists()
    rng = np.random.default_rng(1)
    v = np.vstack([rng.normal([1, 0, 0], 0.05, (10, 3)), rng.normal([0, 1, 0], 0.05, (10, 3)), rng.normal([0, 0, 1], 0.05, (10, 3))])
    l1, l2 = dz.cluster(v), dz.cluster(v)
    assert list(l1) == list(l2) and len(set(l1)) == 3
    assert len(set(dz.cluster(v, num_speakers=2))) == 2


def test_words_get_one_speaker_and_single_flips_are_smoothed():
    words = [{"start_ms": i * 500, "end_ms": i * 500 + 400} for i in range(7)]
    spans = [(0, 2000), (2000, 4000)]
    got = dz.assign_words(words, spans, ["S1", "S2"])
    assert got[:3] == ["S1"] * 3 and got[-2:] == ["S2"] * 2
    flip = dz.assign_words(words[:3], [(0, 600), (600, 1000), (1000, 1500)], ["S1", "S2", "S1"])
    assert flip == ["S1", "S1", "S1"]


def test_utterance_text_low_confidence_and_long_split():
    w = lambda t, a, b, c=1.0: {"text": t, "start_ms": a, "end_ms": b, "conf": c}
    words = [w("你好", 0, 500), w("hello", 600, 900), w("world", 950, 1300, 0.2), w("再见。", 5000, 5500)]
    u = build_utterances(words, ["S1", "S1", "S1", "S1"], offset_ms=100)
    assert [x["text"] for x in u] == ["你好hello world", "再见。"] or [x["text"] for x in u] == ["你好 hello world", "再见。"]
    assert u[0]["t_start_ms"] == 100 and u[1]["word_start"] == 3
    assert join_text([w("a", 0, 1), w("b", 1, 2)]) == "a b" and join_text([w("你", 0, 1), w("b", 1, 2)]) == "你b"
    long_text = "句子。" * 1500
    pieces = split_long(long_text)
    assert "".join(pieces) == long_text and all(len(p) <= 2000 for p in pieces)
    many = [w("字。" * 3, i * 100, i * 100 + 90, 0.5) for i in range(1200)]
    out = build_utterances(many, ["S1"] * 1200)
    assert len(out) >= 2 and all(len(x["text"]) <= 2000 for x in out) and out[0]["low_text_confidence"]
    assert out[0]["word_end"] + 1 == out[1]["word_start"]
