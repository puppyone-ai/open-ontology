"""media-parser backend, run in a per-request child process (doc_parser.supervisor).

`probe` is quick (ffprobe plus the codec verdict). `parse_task` runs the whole file: decode (D65), long audio
(D73), video (D75), and returns the response of `oo.doc-parser/v1` with `items` ordered by the file's time axis.
The job directory `<scratch>/media-jobs/<sha>` keeps the chunk cache between attempts; it is removed when the file
is done. Intermediate WAV, frames and speaker features are deleted as soon as they are used.
"""
import json
import os
import shutil
import time
from pathlib import Path

from doc_parser.supervisor import WorkerError

from . import ADAPTER_VERSION, PROTOCOL
from . import ffmpeg as ff
from .errors import ASR_FAILED, DISK_LOW, with_details
from .meta import parser_version

EXCHANGE_ROOT = os.environ.get("OO_MEDIA_EXCHANGE", "/exchange")
FRAME_BYTES_ESTIMATE = 400_000  # a 1280x720 PNG of a slide; screen recordings are smaller than photos
WAV_BYTES_PER_MS = 32
MARGIN = 256 << 20


def load_engines(opts):
    """`(vad, asr_engine, embedder, models)`; replaced in tests. The image has the models of models.lock.json."""
    from .engines import load

    return load(opts)


def probe(path, fmt):
    info = ff.probe(path)
    restricted = ff.restricted_codecs(info, fmt)
    return json.dumps({
        "container": info["container"], "duration_ms": info["duration_ms"], "start_ms": info["start_ms"],
        "codecs": ff.codecs(info), "restricted_codecs": restricted,
        "decode": "restricted" if restricted else "builtin",
        "has_audio": any(s["type"] == "audio" for s in info["streams"]),
        "has_video": any(s["type"] == "video" and not s["attached_pic"] for s in info["streams"]),
        "size": os.path.getsize(path),
    }).encode()


def ensure_disk(job_dir, need):
    free = shutil.disk_usage(job_dir).free
    if free < need + MARGIN:
        raise with_details(DISK_LOW, "scratch has too little free space for this file", need=need, free=free)


def write_progress(job_dir, done_ms, total_ms, stage):
    p = Path(job_dir) / "progress.json"
    tmp = Path(job_dir) / ".progress.tmp"
    tmp.write_text(json.dumps({"done_ms": int(done_ms), "total_ms": int(total_ms), "stage": stage}))
    os.replace(tmp, p)


def _decoder_for(info, fmt, opts, sha):
    """Which decoder produces the WAV and frames: builtin, host decode output, or the configured external FFmpeg."""
    restricted = ff.restricted_codecs(info, fmt)
    if not restricted:
        return {"kind": "builtin", "ffmpeg": None}
    host = ff.load_host_decode(Path(opts.get("host_decode_dir") or Path(EXCHANGE_ROOT) / sha), sha)
    if host:
        return {"kind": "host", "host": host}
    ext = opts.get("external_ffmpeg")
    if ext and os.path.isabs(ext) and os.access(ext, os.X_OK):
        return {"kind": "external", "ffmpeg": ext}
    raise ff.codec_unavailable(restricted)


def parse_task(path, fmt, opts, max_response, job_dir, sha):
    job = Path(job_dir)
    job.mkdir(parents=True, exist_ok=True)
    info = ff.probe(path)
    ff.check_limits(path, info)
    dec = _decoder_for(info, fmt, opts, sha)
    has_audio = any(s["type"] == "audio" for s in info["streams"])
    has_video = fmt == "video" and any(s["type"] == "video" and not s["attached_pic"] for s in info["streams"])
    need = info["duration_ms"] * WAV_BYTES_PER_MS + (info["duration_ms"] // 1000) * FRAME_BYTES_ESTIMATE * has_video
    ensure_disk(job, need)
    total_ms = max(info["duration_ms"], 1)
    write_progress(job, 0, total_ms, "decode")
    wav = job / "audio.wav"
    frames_dir = job / "frames"
    warnings = []
    try:
        if has_audio:
            if dec["kind"] == "host":
                shutil.copyfile(dec["host"]["wav"], wav)
            else:
                ff.decode_audio(path, wav, dec["ffmpeg"])
        vad, engine, embedder, models = load_engines(opts)
        utterances, speakers, skipped = [], [], None
        if has_audio:
            from asr.pipeline import Pipeline

            pipe = Pipeline(vad, engine, embedder, job / "chunks", job / "emb", hotwords=opts.get("hotwords") or (),
                            num_speakers=opts.get("num_speakers"), media_asset=bool(opts.get("media_asset")),
                            on_chunk=lambda done, tot, i, n, reused: write_progress(job, done, total_ms, "asr"))
            try:
                res = pipe.run(wav, offset_ms=info["start_ms"])
            except WorkerError:
                raise
            except Exception as e:  # noqa: BLE001
                raise with_details(ASR_FAILED, "speech recognition failed", error=type(e).__name__) from None
            utterances, speakers = res["utterances"], res["speakers"]
            skipped = res.get("skipped")
        items = [{"kind": "utterance", **u} for u in utterances]
        if skipped:
            # Not transcribed (a media asset, or no speech): one scene covering the file is the anchor of the Recording's facts.
            items.append({"kind": "scene", "t_start_ms": info["start_ms"], "t_end_ms": info["start_ms"] + info["duration_ms"], "text": "media asset"})
        key_frames = 0
        if has_video:
            write_progress(job, 0, total_ms, "video")
            items, key_frames, vwarn = _video_items(path, dec, info, job, frames_dir, utterances)
            warnings += vwarn
        if not items:
            warnings.append("DOCS_PARSE_EMPTY")
        out = {
            "protocol": PROTOCOL,
            "parser": {"id": "media", "version": parser_version(), "adapter_version": ADAPTER_VERSION},
            "kind": fmt, "container": info["container"], "codec": "+".join(sorted(set(ff.codecs(info)))),
            "duration_ms": info["duration_ms"], "start_ms": info["start_ms"],
            "width": next((s["width"] for s in info["streams"] if s["type"] == "video" and s["width"] and not s["attached_pic"]), None),
            "height": next((s["height"] for s in info["streams"] if s["type"] == "video" and s["height"] and not s["attached_pic"]), None),
            "speakers": speakers, "skipped": skipped, "key_frames": key_frames, "decode": dec["kind"], "items": items,
            "profile": {"tier": "cpu", "isa": _isa(), "models": models}, "warnings": warnings,
        }
        body = json.dumps(out, ensure_ascii=False, sort_keys=True, separators=(",", ":")).encode()
        if len(body) > max_response:
            raise WorkerError("PARSER_OUTPUT_LIMIT", "response is over the limit")
        write_progress(job, total_ms, total_ms, "done")
        shutil.rmtree(job, ignore_errors=True)  # chunk cache, WAV and features are not needed any more
        return body
    finally:
        wav.unlink(missing_ok=True)
        shutil.rmtree(frames_dir, ignore_errors=True)


def _isa():
    from asr.isa import isa_fingerprint

    return isa_fingerprint()


def _video_items(path, dec, info, job, frames_dir, utterances):
    from PIL import Image

    from doc_parser import pictures
    from video import frames as vf

    host_frames = dec["kind"] == "host" and dec["host"]["frames_dir"]
    if host_frames:
        m = dec["host"]["manifest"]
        files = sorted(Path(dec["host"]["frames_dir"]).glob("*.png"))
        pts = m.get("pts_ms") or [i * 1000 for i in range(len(files))]
        if len(pts) != len(files):
            raise with_details("PARSER_HOST_DECODE_MISMATCH", "host decode frame list does not match the manifest")
    else:
        files = ff.extract_frames(path, frames_dir, dec.get("ffmpeg"))
        pts = [i * 1000 for i in range(len(files))]
    frames = [(t, Image.open(f).convert("RGB")) for t, f in zip(pts, files)]
    res = vf.analyse(frames, info["duration_ms"], pictures.read_lines, utterances, offset_ms=info["start_ms"])
    return res["items"], res["key_frames"], res["warnings"]
