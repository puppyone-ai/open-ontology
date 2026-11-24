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
PAGE_RANGE_FORMATS = ("pdf",)


def parser_version():
    try:
        from importlib import metadata

        return metadata.version(PACKAGE)
    except Exception:  # noqa: BLE001 - tests run without docling
        return "unavailable"


def ocr_allowed():
    """Explicit RapidOCR (B-S9-04): on only when the rapidocr package and its pinned models are in the image."""
    from .pictures import ocr_available

    return ocr_available()


def limits():
    return {"ocr": ocr_allowed(), "table_structure": table_structure_available()}


def licenses(models):
    """`/v1/info.licenses`: components and models for the SBOM gate to compare (10 §17.1)."""
    return [{"name": m["name"], "sha256": m["sha256"]} for m in models]
