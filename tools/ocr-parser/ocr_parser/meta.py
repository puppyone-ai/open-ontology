"""ocr-parser identity: light module, no Paddle import."""
PARSER_ID = "paddleocr"
FORMATS = ("pdf", "image")
PROBE_FORMATS = ()
PROFILE = "cpu"
OCR_OPTION_ALLOWED = True
PAGE_RANGE_FORMATS = ("pdf",)
# Weights that decide the output; the rest of the lock (configs, READMEs) is not listed in the IR profile.
WEIGHT_SUFFIX = "inference.pdiparams"


def parser_version():
    try:
        from importlib import metadata

        return metadata.version("paddleocr")
    except Exception:  # noqa: BLE001 - dev hosts have no paddleocr
        return "unavailable"


def limits():
    return {"ocr": True, "table_structure": False, "max_pages_per_request": 20, "dpi": 200}


def licenses(models):
    """Components and models with SPDX ids for the SBOM gate (10 §17.1)."""
    out = [{"name": m["name"], "sha256": m["sha256"], "spdx": "Apache-2.0"} for m in models if m["name"].endswith(WEIGHT_SUFFIX)]
    for name, version, spdx in (("paddleocr", parser_version(), "Apache-2.0"),):
        out.append({"name": name, "version": version, "spdx": spdx})
    return out
