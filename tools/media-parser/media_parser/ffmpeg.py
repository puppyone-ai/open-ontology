"""FFmpeg access and the codec policy of D65 (10 section 17.2).

Our own LGPL FFmpeg decodes only what is in `BUILTIN_DECODABLE`. A file whose codec is outside it (H.264,
HEVC, AAC, ...) is decoded elsewhere: macOS Desktop hands over WAV and frames with a manifest (host decode),
or a Linux customer configures `media.external_ffmpeg`. With neither: `PARSER_CODEC_UNAVAILABLE` naming the
codec and how to configure. Intermediate files live in the job scratch and are deleted by the caller.
"""
import hashlib
import json
import os
import subprocess
from pathlib import Path

from .errors import CODEC_UNAVAILABLE, HOST_DECODE_MISMATCH, HOW_TO_CONFIGURE, MEDIA_TOO_LONG, with_details
from .meta import MAX_BYTES, MAX_DURATION_MS

FFMPEG = os.environ.get("OO_FFMPEG", "ffmpeg")
FFPROBE = os.environ.get("OO_FFPROBE", "ffprobe")
# ffprobe codec names our build can decode (the decoder allow-list of Dockerfile.ffmpeg).
BUILTIN_DECODABLE = {
    "opus", "vorbis", "flac", "mp3", "vp8", "vp9", "av1",
    "pcm_s16le", "pcm_s16be", "pcm_s24le", "pcm_s24be", "pcm_s32le", "pcm_f32le", "pcm_f64le", "pcm_u8", "pcm_alaw",
    "pcm_mulaw",
    # still images used as the video track of an audio-only container (cover art): never decoded here
    "png", "mjpeg",
}
RESTRICTED_NAMES = {"h264", "hevc", "aac", "mpeg4", "mpeg2video", "wmv3", "vc1", "ac3", "eac3", "alac", "wmav2"}
EXCHANGE_MANIFEST = "manifest.json"


class FFmpegError(Exception):
    pass


def run(cmd, timeout=None):
    p = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE, timeout=timeout)
    if p.returncode != 0:
        raise FFmpegError(p.stderr.decode("utf-8", "replace")[-500:])
    return p.stdout


def probe(path, ffprobe=None):
    """Container and tracks: `{"duration_ms", "start_ms", "container", "streams": [{index, type, codec, ...}]}`."""
    out = run([ffprobe or FFPROBE, "-v", "error", "-show_format", "-show_streams", "-of", "json", str(path)], 60)
    j = json.loads(out)
    fmt = j.get("format", {})
    streams = []
    for s in j.get("streams", []):
        streams.append({
            "index": s.get("index"), "type": s.get("codec_type"), "codec": s.get("codec_name", "unknown"),
            "sample_rate": int(s["sample_rate"]) if s.get("sample_rate") else None,
            "channels": s.get("channels"), "width": s.get("width"), "height": s.get("height"),
            "duration_s": float(s["duration"]) if s.get("duration") not in (None, "N/A") else None,
            "attached_pic": bool(s.get("disposition", {}).get("attached_pic")),
        })
    dur = float(fmt["duration"]) if fmt.get("duration") not in (None, "N/A") else None
    if dur is None:
        durs = [s["duration_s"] for s in streams if s["duration_s"]]
        dur = max(durs) if durs else 0.0
    return {
        "duration_ms": int(round(dur * 1000)),
        "start_ms": int(round(float(fmt.get("start_time") or 0) * 1000)),
        "container": fmt.get("format_name", ""),
        "size": int(fmt["size"]) if fmt.get("size") else None,
        "streams": streams,
    }


def codecs(info, kind=None):
    return [s["codec"] for s in info["streams"] if (kind is None or s["type"] == kind) and not s["attached_pic"]]


def restricted_codecs(info, kind):
    """Codecs of the wanted track kinds that our build cannot decode."""
    kinds = ("audio", "video") if kind == "video" else ("audio",)
    return sorted({s["codec"] for s in info["streams"]
                   if s["type"] in kinds and not s["attached_pic"] and s["codec"] not in BUILTIN_DECODABLE})


def check_limits(path, info):
    size = os.path.getsize(path)
    if size > MAX_BYTES or info["duration_ms"] > MAX_DURATION_MS:
        raise with_details(MEDIA_TOO_LONG, "media is over 4 GiB or 4 hours",
                           size=size, duration_ms=info["duration_ms"], max_bytes=MAX_BYTES, max_duration_ms=MAX_DURATION_MS)


def codec_unavailable(codecs_):
    return with_details(CODEC_UNAVAILABLE, "no decoder for " + ", ".join(codecs_), codecs=list(codecs_),
                        how_to=HOW_TO_CONFIGURE)


def sha256_file(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def load_host_decode(exchange_dir, blob_sha256):
    """Host decode helper output (macOS Desktop, 20 F-S10-05): `<dir>/manifest.json` + `audio.wav` (+ frames).

    The manifest names the blob it was made from; a different hash means the file changed after the helper ran,
    and the output is dropped (`PARSER_HOST_DECODE_MISMATCH`, the engine queues the file again).
    """
    d = Path(exchange_dir)
    mf = d / EXCHANGE_MANIFEST
    if not mf.is_file():
        return None
    try:
        m = json.loads(mf.read_text())
    except ValueError:
        raise with_details(HOST_DECODE_MISMATCH, "host decode manifest is not JSON") from None
    if m.get("source_blob_sha256") != blob_sha256:
        raise with_details(HOST_DECODE_MISMATCH, "host decode output belongs to another version of the file",
                           helper_version=m.get("helper_version"))
    wav = d / m.get("audio", "audio.wav")
    if not wav.is_file():
        raise with_details(HOST_DECODE_MISMATCH, "host decode output has no audio")
    for want, name in (m.get("files") or {}).items():
        if sha256_file(d / name) != want:
            raise with_details(HOST_DECODE_MISMATCH, "host decode file hash differs from the manifest")
    return {"wav": wav, "manifest": m, "frames_dir": d / "frames" if (d / "frames").is_dir() else None}


def decode_audio(src, dst_wav, ffmpeg=None, audio_index=None):
    """16 kHz mono 16-bit WAV. Timestamps of the WAV start at 0; add `info["start_ms"]` for the source axis."""
    cmd = [ffmpeg or FFMPEG, "-nostdin", "-v", "error", "-y", "-i", str(src)]
    cmd += ["-map", f"0:{audio_index}" if audio_index is not None else "0:a:0", "-vn", "-sn", "-dn",
            "-ac", "1", "-ar", "16000", "-c:a", "pcm_s16le", str(dst_wav)]
    run(cmd)
    return Path(dst_wav)


def extract_frames(src, out_dir, ffmpeg=None, fps=1):
    """1 fps PNG frames `out_dir/%08d.png`; frame n (1-based) is at `(n-1) / fps` seconds from the start."""
    Path(out_dir).mkdir(parents=True, exist_ok=True)
    run([ffmpeg or FFMPEG, "-nostdin", "-v", "error", "-y", "-i", str(src), "-map", "0:v:0", "-an", "-sn", "-dn",
         "-vf", f"fps={fps}", "-vsync", "0", "-f", "image2", str(Path(out_dir) / "%08d.png")])
    return sorted(Path(out_dir).glob("*.png"))


def buildconf(ffmpeg=None):
    p = subprocess.run([ffmpeg or FFMPEG, "-hide_banner", "-buildconf"], stdout=subprocess.PIPE, stderr=subprocess.STDOUT)
    return p.stdout.decode("utf-8", "replace")


def decoders(ffmpeg=None):
    """Names of the decoders compiled in (`ffmpeg -decoders`)."""
    out = subprocess.run([ffmpeg or FFMPEG, "-hide_banner", "-decoders"], stdout=subprocess.PIPE).stdout.decode()
    names = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and len(parts[0]) == 6 and parts[0][0] in "VAS" and set(parts[0][1:]) <= set("FSXBD.") and parts[1] != "=":
            names.append(parts[1])
    return names
