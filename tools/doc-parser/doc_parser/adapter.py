"""Docling backend: `probe` and `parse`, run inside a per-request child process.

Docling is imported lazily inside the functions so that importing this module
in the fork server (warm start) pulls in the heavy modules once, while the HTTP
process itself never loads them. L3 has no OCR: `do_ocr=False`, no picture,
code or formula enrichment, no remote services, models only from
`DOCLING_ARTIFACTS_PATH` (offline).
"""
import json
import os
import tempfile
from pathlib import Path

from . import ADAPTER_VERSION, PROTOCOL
from .capabilities import table_structure_available
from .supervisor import WorkerError

MODELS_DIR = os.environ.get("DOCLING_ARTIFACTS_PATH", "/opt/models")
OLE_MAGIC = b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1"
TEXT_LAYER_MIN_CHARS = 10


def _preload():
    """Import the heavy modules in the fork server so workers start warm."""
    try:
        import docling.datamodel.base_models  # noqa: F401
        import docling.datamodel.pipeline_options  # noqa: F401
        import docling.document_converter  # noqa: F401
        import pypdfium2  # noqa: F401
    except Exception:  # noqa: BLE001 - tests and dev hosts have no docling
        pass


_preload()


def parser_version():
    from importlib import metadata

    return metadata.version("docling-slim")


def _open_pdf(path):
    import pypdfium2 as pdfium

    try:
        return pdfium.PdfDocument(str(path)), False
    except pdfium.PdfiumError as e:
        if "password" in str(e).lower():
            return None, True
        raise WorkerError("PARSER_CORRUPT", "pdf cannot be opened") from None


def _has_encrypt_marker(path):
    size = os.path.getsize(path)
    with open(path, "rb") as f:
        head = f.read(4096)
        f.seek(max(0, size - 4096))
        tail = f.read(4096)
    return b"/Encrypt" in head or b"/Encrypt" in tail


def probe(path, fmt):
    doc, needs_password = _open_pdf(path)
    encrypted = needs_password or _has_encrypt_marker(path)
    if doc is None:
        return json.dumps(
            {"page_count": 0, "encrypted": True, "needs_password": True, "text_layer_pages": 0}
        ).encode()
    try:
        pages = len(doc)
        text_pages = 0
        for i in range(pages):
            page = doc[i]
            try:
                tp = page.get_textpage()
                try:
                    if tp.count_chars() >= TEXT_LAYER_MIN_CHARS:
                        text_pages += 1
                finally:
                    tp.close()
            finally:
                page.close()
    finally:
        doc.close()
    return json.dumps(
        {"page_count": pages, "encrypted": encrypted, "needs_password": False, "text_layer_pages": text_pages}
    ).encode()


def _converter(table_structure):
    from docling.datamodel.base_models import InputFormat
    from docling.datamodel.pipeline_options import AcceleratorOptions, PdfPipelineOptions
    from docling.document_converter import DocumentConverter, PdfFormatOption

    opts = PdfPipelineOptions(
        artifacts_path=MODELS_DIR,
        do_ocr=False,
        do_table_structure=bool(table_structure) and table_structure_available(),
        do_picture_classification=False,
        do_picture_description=False,
        do_code_enrichment=False,
        do_formula_enrichment=False,
        generate_page_images=False,
        generate_picture_images=False,
        generate_table_images=False,
        enable_remote_services=False,
        allow_external_plugins=False,
        accelerator_options=AcceleratorOptions(num_threads=2, device="cpu"),
    )
    return DocumentConverter(
        allowed_formats=[InputFormat.PDF, InputFormat.DOCX, InputFormat.PPTX],
        format_options={InputFormat.PDF: PdfFormatOption(pipeline_options=opts)},
    )


def _precheck(path, fmt):
    with open(path, "rb") as f:
        head = f.read(8)
    if fmt in ("docx", "pptx"):
        if head == OLE_MAGIC:
            # Encrypted OOXML is an OLE2 wrapper.
            data = Path(path).read_bytes()[: 32 << 20]
            enc = "EncryptedPackage".encode("utf-16-le") in data
            raise WorkerError("PARSER_ENCRYPTED" if enc else "PARSER_UNSUPPORTED",
                              "legacy or encrypted office file")
        if head[:4] != b"PK\x03\x04":
            raise WorkerError("PARSER_CORRUPT", "not an OOXML package")
    else:
        if not head.startswith(b"%PDF-"):
            raise WorkerError("PARSER_CORRUPT", "not a pdf")
        doc, needs_password = _open_pdf(path)
        if needs_password:
            raise WorkerError("PARSER_ENCRYPTED", "password required")
        doc.close()
        if _has_encrypt_marker(path):
            raise WorkerError("PARSER_ENCRYPTED", "pdf is encrypted")


def parse(path, fmt, page_range, table_structure, max_response):
    _precheck(path, fmt)
    from docling.exceptions import ConversionError

    with tempfile.TemporaryDirectory(prefix="job-") as tmp:
        link = Path(tmp) / f"document.{fmt}"
        os.symlink(path, link)
        kwargs = {"page_range": (page_range[0], page_range[1])} if page_range else {}
        try:
            result = _converter(table_structure).convert(link, raises_on_error=True, **kwargs)
        except ConversionError as e:
            raise WorkerError("PARSER_CORRUPT", f"conversion failed: {type(e).__name__}") from None
        doc = result.document.export_to_dict()
        page_count = int(getattr(result.input, "page_count", 0) or 0)
    warnings = []
    if fmt == "pdf" and table_structure and not table_structure_available():
        warnings.append("DOCS_PARSE_TABLE_STRUCTURE_OFF")
    for err in getattr(result, "errors", []) or []:
        warnings.append("DOCS_PARSE_ENGINE_WARNING")
        break
    out = {
        "protocol": PROTOCOL,
        "parser": {"id": "docling", "version": parser_version(), "adapter_version": ADAPTER_VERSION},
        "page_count": page_count,
        "pages": [page_range[0], min(page_range[1], page_count) if page_count else page_range[1]]
        if page_range
        else [1, page_count],
        "doc": doc,
        "warnings": warnings,
    }
    if fmt == "docx":
        from .ooxml import docx_facts

        try:
            out["ooxml"] = docx_facts(path)
        except Exception:  # noqa: BLE001 - alignment falls back with a warning
            out["warnings"].append("DOCS_PARSE_DOCX_NO_OOXML")
    body = json.dumps(out, ensure_ascii=False, separators=(",", ":")).encode()
    if len(body) > max_response:
        raise WorkerError("PARSER_OUTPUT_LIMIT", "response is over the limit")
    return body
