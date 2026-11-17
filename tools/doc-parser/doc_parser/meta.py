"""What the doc-parser sidecar is: formats, parser id, limits. Light module (no Docling import).

Other sidecars that reuse the HTTP server (`ocr-parser`, `media-parser`) ship their own meta
module and select it with OO_PARSER_META.
"""
from .capabilities import table_structure_available

PARSER_ID = "docling"
PACKAGE = "docling-slim"
FORMATS = ("pdf", "docx", "pptx")
PROBE_FORMATS = ("pdf",)
PROFILE = "cpu"
OCR_OPTION_ALLOWED = False  # explicit OCR is on in the image since B-S9-04 only when the lock has RapidOCR
PAGE_RANGE_FORMATS = ("pdf",)


def parser_version():
    try:
        from importlib import metadata

        return metadata.version(PACKAGE)
    except Exception:  # noqa: BLE001 - tests run without docling
        return "unavailable"


def limits():
    return {"ocr": OCR_OPTION_ALLOWED, "table_structure": table_structure_available()}


def licenses(models):
    """`/v1/info.licenses`: components and models for the SBOM gate to compare (10 §17.1)."""
    return [{"name": m["name"], "sha256": m["sha256"]} for m in models]
