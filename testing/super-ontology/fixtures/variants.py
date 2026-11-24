"""Content variants of the golden files, for the L3 latency runs and the 1000
file bench (10 section 12.9: "golden expanded to 1000 by script copy and
mutation"). Stdlib only, deterministic: the same (template, n) gives the same
bytes, and different n give different bytes (so different content hashes).

Every variant changes real content, not just a trailer, so it is parsed for
real: a text node of the first body part for md/txt/csv/docx/pptx/xlsx.
PDF layouts are hand written with absolute offsets, so a PDF variant appends an
incremental-update comment after %%EOF (valid to readers, new bytes, same
page content); the parse cost is the real parse cost.
"""

import io
import re
import zipfile


def _tag(n: int) -> str:
    return f"变体{n:05d}"


def md(data: bytes, n: int) -> bytes:
    text = data.decode("utf-8")
    return (text.rstrip("\n") + f"\n\n备注：{_tag(n)}\n").encode("utf-8")


def txt(data: bytes, n: int) -> bytes:
    return md(data, n)


def csv(data: bytes, n: int) -> bytes:
    text = data.decode("utf-8")
    sep = "\t" if text.count("\t") > text.count(",") else ","
    return (text.rstrip("\n") + f"\n{_tag(n)}{sep}{n}\n").encode("utf-8")


def pdf(data: bytes, n: int) -> bytes:
    return data + f"\n%% oo-variant {n:05d}\n".encode("ascii")


def _first_text_node(xml: str, pattern: str, n: int):
    m = re.search(pattern, xml)
    if not m:
        return None
    start, end = m.span(2)
    return xml[:start] + m.group(2) + " " + _tag(n) + xml[end:]


def _ooxml(data: bytes, n: int, parts: list) -> bytes:
    """Rewrite the zip with one text node of the first matching part extended."""
    src = zipfile.ZipFile(io.BytesIO(data))
    out = io.BytesIO()
    done = False
    with zipfile.ZipFile(out, "w") as dst:
        for info in src.infolist():
            blob = src.read(info.filename)
            if not done:
                for name_re, node_re in parts:
                    if re.fullmatch(name_re, info.filename):
                        changed = _first_text_node(blob.decode("utf-8"), node_re, n)
                        if changed is not None:
                            blob = changed.encode("utf-8")
                            done = True
                            break
            zi = zipfile.ZipInfo(info.filename, info.date_time)
            zi.compress_type = info.compress_type
            zi.external_attr = info.external_attr
            zi.create_system = info.create_system
            dst.writestr(zi, blob)
    if not done:
        raise ValueError("no text node to vary")
    return out.getvalue()


def docx(data: bytes, n: int) -> bytes:
    return _ooxml(data, n, [(r"word/document\.xml", r"(<w:t(?: [^>]*)?>)([^<]+)(</w:t>)")])


def pptx(data: bytes, n: int) -> bytes:
    return _ooxml(data, n, [(r"ppt/slides/slide1\.xml", r"(<a:t>)([^<]+)(</a:t>)"),
                            (r"ppt/slides/slide\d+\.xml", r"(<a:t>)([^<]+)(</a:t>)")])


def xlsx(data: bytes, n: int) -> bytes:
    return _ooxml(data, n, [(r"xl/sharedStrings\.xml", r"(<t(?: [^>]*)?>)([^<]+)(</t>)"),
                            (r"xl/worksheets/sheet\d+\.xml", r"(<t(?: [^>]*)?>)([^<]+)(</t>)")])


MAKERS = {"md": md, "txt": txt, "csv": csv, "pdf": pdf, "docx": docx, "pptx": pptx, "xlsx": xlsx}


def vary(ext: str, data: bytes, n: int) -> bytes:
    return MAKERS[ext](data, n)
