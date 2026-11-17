"""Typed failures of the media backend. `code` is the sidecar error code; the engine maps it to `DOCS_*`."""
import json

from doc_parser.supervisor import WorkerError

CODEC_UNAVAILABLE = "PARSER_CODEC_UNAVAILABLE"      # DOCS_PARSE_CODEC_UNAVAILABLE
MEDIA_TOO_LONG = "PARSER_MEDIA_TOO_LONG"            # DOCS_SOURCE_MEDIA_TOO_LONG
HOST_DECODE_MISMATCH = "PARSER_HOST_DECODE_MISMATCH"  # DOCS_PARSE_MEDIA_HOST_DECODE_MISMATCH
ASR_FAILED = "PARSER_ASR_FAILED"                    # DOCS_PARSE_ASR_FAILED
DIARIZATION_FAILED = "PARSER_DIARIZATION_FAILED"    # DOCS_PARSE_DIARIZATION_FAILED
DISK_LOW = "PARSER_DISK_LOW"                        # blocked_reason=disk_low

HOW_TO_CONFIGURE = (
    "macOS: the Desktop app decodes H.264, HEVC and AAC files with the system frameworks. Linux: set "
    "media.external_ffmpeg in the oo.docs/v1 configuration to the absolute path of your own FFmpeg."
)


def with_details(code, message, **details):
    """A WorkerError whose message is JSON `{"message", "details"}` (the HTTP layer unpacks it)."""
    return WorkerError(code, json.dumps({"message": message, "details": details}, ensure_ascii=False))


def split_message(message):
    try:
        v = json.loads(message)
        if isinstance(v, dict) and "details" in v:
            return v.get("message", ""), v["details"]
    except (TypeError, ValueError):
        pass
    return message, None
