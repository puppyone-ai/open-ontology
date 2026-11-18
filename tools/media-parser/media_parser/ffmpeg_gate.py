#!/usr/bin/env python3
"""CI gate for our FFmpeg build (90 section 4, 10 section 17.2, D65).

    python3 -B -m media_parser.ffmpeg_gate [/path/to/ffmpeg]

Fails when `ffmpeg -buildconf` shows --enable-gpl, --enable-nonfree or --enable-version3, when any forbidden
decoder (h264, hevc, aac, ...) is compiled in, when a decoder outside the allow-list is present, or when an
encoder other than pcm_s16le and png is present.
"""
import re
import sys

from . import ffmpeg as ff

ALLOWED_DECODERS = {
    "opus", "libopus_never", "vorbis", "flac", "mp3", "mp3float", "vp8", "vp9", "libdav1d",
    "pcm_s16le", "pcm_s16be", "pcm_s24le", "pcm_s24be", "pcm_s32le", "pcm_f32le", "pcm_f64le", "pcm_u8", "pcm_alaw", "pcm_mulaw",
}
FORBIDDEN_DECODERS = {"h264", "hevc", "aac", "aac_fixed", "aac_latm", "mpeg4", "mpeg2video", "wmv3", "vc1", "ac3", "eac3", "alac",
                      "h263", "mpeg1video", "wmv1", "wmv2", "msmpeg4v3", "libfdk_aac", "h264_videotoolbox", "hevc_videotoolbox"}
FORBIDDEN_FLAGS = ("--enable-gpl", "--enable-nonfree", "--enable-version3", "--enable-libx264", "--enable-libx265",
                   "--enable-libfdk-aac", "--enable-libfdk_aac")
ALLOWED_ENCODERS = {"pcm_s16le", "png"}


def check_buildconf(text):
    """Problems found in the configuration lines of `-buildconf`."""
    problems = []
    for flag in FORBIDDEN_FLAGS:
        if re.search(re.escape(flag) + r"(?![\w-])", text):
            problems.append(f"configure has {flag}")
    if "--disable-gpl" not in text or "--disable-nonfree" not in text:
        problems.append("configure must say --disable-gpl and --disable-nonfree")
    if "--disable-everything" not in text:
        problems.append("configure must start from --disable-everything (allow-list build)")
    return problems


def check_decoders(names):
    problems = []
    bad = sorted(set(names) & FORBIDDEN_DECODERS)
    if bad:
        problems.append("forbidden decoders present: " + ", ".join(bad))
    extra = sorted(set(names) - ALLOWED_DECODERS - FORBIDDEN_DECODERS)
    if extra:
        problems.append("decoders outside the allow-list: " + ", ".join(extra))
    return problems


def check_encoders(names):
    extra = sorted(set(names) - ALLOWED_ENCODERS)
    return ["encoders outside pcm_s16le and png: " + ", ".join(extra)] if extra else []


def encoders(ffmpeg=None):
    import subprocess

    out = subprocess.run([ffmpeg or ff.FFMPEG, "-hide_banner", "-encoders"], stdout=subprocess.PIPE).stdout.decode()
    names = []
    for line in out.splitlines():
        parts = line.split()
        if len(parts) >= 2 and len(parts[0]) == 6 and parts[0][0] in "VAS" and set(parts[0][1:]) <= set("FSXBD.") and parts[1] != "=":
            names.append(parts[1])
    return names


def main(argv):
    exe = argv[1] if len(argv) > 1 else ff.FFMPEG
    problems = check_buildconf(ff.buildconf(exe)) + check_decoders(ff.decoders(exe)) + check_encoders(encoders(exe))
    for p in problems:
        print("FFMPEG GATE FAIL:", p)
    print("FFMPEG GATE:", "FAIL" if problems else "PASS")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
