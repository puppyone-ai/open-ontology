"""media-parser identity: light module, no heavy import."""
import os

PARSER_ID = "media"
FORMATS = ("audio", "video")
PROBE_FORMATS = ("audio", "video")
PROFILE = os.environ.get("OO_MEDIA_PROFILE", "cpu")
PAGE_RANGE_FORMATS = ()
MAX_BYTES = 4 << 30
MAX_DURATION_MS = 4 * 3600 * 1000


def ocr_allowed():
    return True


def parser_version():
    return os.environ.get("OO_MEDIA_VERSION", "1")


def limits():
    return {"ocr": True, "max_media_bytes": MAX_BYTES, "max_duration_ms": MAX_DURATION_MS, "long_tasks": True,
            "max_keyframes_per_hour": 600}


def licenses(models):
    """Components and models with SPDX ids for the SBOM gate (10 section 17.1)."""
    from .components import COMPONENTS

    out = [dict(c) for c in COMPONENTS]
    out += [{"name": m["name"], "sha256": m["sha256"]} for m in models]
    return out
