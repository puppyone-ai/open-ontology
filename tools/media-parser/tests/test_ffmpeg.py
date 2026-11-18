import json
import os
import shutil
import subprocess
import wave
from pathlib import Path

import numpy as np
import pytest

from doc_parser.supervisor import WorkerError
from media_parser import adapter, errors
from media_parser import ffmpeg as ff
from media_parser import ffmpeg_gate as gate

HAVE_FFMPEG = shutil.which("ffmpeg") is not None and shutil.which("ffprobe") is not None
needs_ffmpeg = pytest.mark.skipif(not HAVE_FFMPEG, reason="needs ffmpeg and ffprobe on PATH (the image has our LGPL build)")

GOOD_CONF = "--disable-gpl --disable-nonfree --disable-everything --enable-decoder=opus,flac --enable-shared"


def make(tmp_path, name, args, dur=3):
    out = tmp_path / name
    cmd = ["ffmpeg", "-nostdin", "-v", "error", "-y", "-f", "lavfi", "-i", f"sine=frequency=440:duration={dur}:sample_rate=44100"] + args + [str(out)]
    subprocess.run(cmd, check=True)
    return out


def test_gate_accepts_an_allow_list_build_and_rejects_gpl_nonfree_and_codecs():
    assert gate.check_buildconf(GOOD_CONF) == []
    assert gate.check_buildconf("--enable-gpl --disable-gpl --disable-nonfree --disable-everything")
    assert any("nonfree" in p for p in gate.check_buildconf("--enable-nonfree --disable-gpl --disable-nonfree --disable-everything"))
    assert gate.check_buildconf("--disable-gpl --disable-nonfree --enable-libx264")  # also no disable-everything
    assert any("version3" in p for p in gate.check_buildconf("--enable-version3 --disable-gpl --disable-nonfree --disable-everything"))
    assert gate.check_decoders(["opus", "vp9", "libdav1d", "pcm_s16le", "flac"]) == []
    assert "h264" in gate.check_decoders(["opus", "h264"])[0] and "aac" in gate.check_decoders(["aac"])[0]
    assert gate.check_decoders(["opus", "wavpack"])[0].startswith("decoders outside")
    assert gate.check_encoders(["pcm_s16le", "png"]) == [] and gate.check_encoders(["libx264"])


def test_codec_verdict_uses_the_allow_list():
    info = {"streams": [{"type": "audio", "codec": "aac", "attached_pic": False}, {"type": "video", "codec": "h264", "attached_pic": False},
                        {"type": "video", "codec": "mjpeg", "attached_pic": True}]}
    assert ff.restricted_codecs(info, "video") == ["aac", "h264"]
    assert ff.restricted_codecs(info, "audio") == ["aac"], "an audio job ignores the video track"
    ok = {"streams": [{"type": "audio", "codec": "opus", "attached_pic": False}, {"type": "video", "codec": "vp9", "attached_pic": False}]}
    assert ff.restricted_codecs(ok, "video") == []
    for c in ("h264", "hevc", "aac"):
        assert c not in ff.BUILTIN_DECODABLE


def test_limits_and_disk_guard(tmp_path, monkeypatch):
    f = tmp_path / "x.bin"
    f.write_bytes(b"0")
    ff.check_limits(f, {"duration_ms": 4 * 3600 * 1000})
    with pytest.raises(WorkerError) as e:
        ff.check_limits(f, {"duration_ms": 4 * 3600 * 1000 + 1})
    assert e.value.code == errors.MEDIA_TOO_LONG
    monkeypatch.setattr(os.path, "getsize", lambda p: (4 << 30) + 1)
    with pytest.raises(WorkerError) as e:
        ff.check_limits(f, {"duration_ms": 1000})
    assert e.value.code == errors.MEDIA_TOO_LONG and errors.split_message(e.value.message)[1]["max_bytes"] == 4 << 30
    monkeypatch.undo()
    monkeypatch.setattr(shutil, "disk_usage", lambda p: shutil._ntuple_diskusage(100, 99, 1 << 20))
    with pytest.raises(WorkerError) as e:
        adapter.ensure_disk(tmp_path, 10 << 20)
    assert e.value.code == errors.DISK_LOW


def test_host_decode_manifest_must_match_the_blob(tmp_path):
    d = tmp_path / "ex"
    d.mkdir()
    assert ff.load_host_decode(d, "a" * 64) is None
    (d / "audio.wav").write_bytes(b"RIFF")
    (d / "manifest.json").write_text(json.dumps({"source_blob_sha256": "a" * 64, "helper_version": "1", "audio": "audio.wav",
                                                 "pts_ms": [0, 1000]}))
    got = ff.load_host_decode(d, "a" * 64)
    assert got["wav"].name == "audio.wav" and got["manifest"]["helper_version"] == "1"
    with pytest.raises(WorkerError) as e:
        ff.load_host_decode(d, "b" * 64)
    assert e.value.code == errors.HOST_DECODE_MISMATCH
    (d / "manifest.json").write_text(json.dumps({"source_blob_sha256": "a" * 64, "audio": "audio.wav", "files": {"0" * 64: "audio.wav"}}))
    with pytest.raises(WorkerError) as e:
        ff.load_host_decode(d, "a" * 64)
    assert e.value.code == errors.HOST_DECODE_MISMATCH


@needs_ffmpeg
def test_probe_decode_and_start_offset(tmp_path):
    wav = make(tmp_path, "a.wav", ["-ac", "2", "-c:a", "pcm_s16le"])
    info = ff.probe(wav)
    assert info["duration_ms"] in range(2950, 3100) and ff.codecs(info) == ["pcm_s16le"]
    assert ff.restricted_codecs(info, "audio") == []
    out = ff.decode_audio(wav, tmp_path / "o.wav")
    with wave.open(str(out)) as w:
        assert (w.getnchannels(), w.getframerate(), w.getsampwidth()) == (1, 16000, 2)
        assert abs(w.getnframes() / 16000 - 3.0) < 0.1
    assert json.loads(adapter.probe(wav, "audio"))["decode"] == "builtin"


@needs_ffmpeg
def test_aac_in_mp4_is_restricted_and_ends_as_codec_unavailable_without_a_decoder_path(tmp_path):
    mp4 = make(tmp_path, "a.m4a", ["-c:a", "aac"])
    assert ff.restricted_codecs(ff.probe(mp4), "audio") == ["aac"]
    p = json.loads(adapter.probe(mp4, "audio"))
    assert p["decode"] == "restricted" and p["restricted_codecs"] == ["aac"]
    with pytest.raises(WorkerError) as e:
        adapter.parse_task(str(mp4), "audio", {}, 1 << 20, str(tmp_path / "job"), "c" * 64)
    assert e.value.code == errors.CODEC_UNAVAILABLE
    msg, details = errors.split_message(e.value.message)
    assert details["codecs"] == ["aac"] and "media.external_ffmpeg" in details["how_to"] and "macOS" in details["how_to"]
    assert not (tmp_path / "job" / "audio.wav").exists()


class FakeVad:
    def probs(self, x):
        n = len(x) // 512
        return [1.0 if float(np.sqrt(np.mean(x[i * 512:(i + 1) * 512] ** 2))) > 0.05 else 0.0 for i in range(n)]


class FakeEngine:
    revision, params = "fake/1", {}

    def transcribe(self, x, hotwords=()):
        return [{"text": "你好", "start_ms": 0, "end_ms": int(len(x) * 1000 / 16000), "conf": 0.9}]


class FakeEmbedder:
    def embed(self, x):
        return np.array([1.0, 0.0], dtype=np.float32)


@pytest.fixture
def fake_engines(monkeypatch):
    monkeypatch.setattr(adapter, "load_engines", lambda opts: (FakeVad(), FakeEngine(), FakeEmbedder(), [{"name": "fake", "sha256": "0" * 64}]))


@needs_ffmpeg
def test_external_ffmpeg_path_decodes_a_restricted_codec(tmp_path, fake_engines):
    m4a = make(tmp_path, "a.m4a", ["-c:a", "aac"])
    body = adapter.parse_task(str(m4a), "audio", {"external_ffmpeg": shutil.which("ffmpeg")}, 1 << 24, str(tmp_path / "job"), "d" * 64)
    out = json.loads(body)
    assert out["decode"] == "external" and out["kind"] == "audio" and out["profile"]["tier"] == "cpu"
    assert out["speakers"] == ["S1"] and out["items"][0]["kind"] == "utterance"
    assert not (tmp_path / "job").exists(), "job directory (chunk cache, WAV, features) is removed when done"
    # a relative or missing path is not accepted
    with pytest.raises(WorkerError) as e:
        adapter.parse_task(str(m4a), "audio", {"external_ffmpeg": "ffmpeg"}, 1 << 24, str(tmp_path / "job2"), "d" * 64)
    assert e.value.code == errors.CODEC_UNAVAILABLE


@needs_ffmpeg
def test_host_decode_output_replaces_the_container_decode(tmp_path, fake_engines):
    m4a = make(tmp_path, "a.m4a", ["-c:a", "aac"])
    sha = ff.sha256_file(m4a)
    ex = tmp_path / "ex" / sha
    ex.mkdir(parents=True)
    ff.decode_audio(m4a, ex / "audio.wav")
    (ex / "manifest.json").write_text(json.dumps({"source_blob_sha256": sha, "helper_version": "avf-1", "audio": "audio.wav"}))
    out = json.loads(adapter.parse_task(str(m4a), "audio", {"host_decode_dir": str(ex)}, 1 << 24, str(tmp_path / "job"), sha))
    assert out["decode"] == "host"
    (ex / "manifest.json").write_text(json.dumps({"source_blob_sha256": "f" * 64, "audio": "audio.wav"}))
    with pytest.raises(WorkerError) as e:
        adapter.parse_task(str(m4a), "audio", {"host_decode_dir": str(ex)}, 1 << 24, str(tmp_path / "job3"), sha)
    assert e.value.code == errors.HOST_DECODE_MISMATCH
