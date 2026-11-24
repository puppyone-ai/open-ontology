"""Body paragraph facts of a docx, read straight from `word/document.xml`.

Docling gives no provenance for DOCX and its item order is not a promise
(10 §14), so the paragraph ordinals of §3.5 are computed here: the 0-based
index among the direct `w:p` children of `w:body`. Paragraphs inside tables
are not numbered; a table records how many body paragraphs precede it.
"""
import re
import zipfile

from lxml import etree

W = "{http://schemas.openxmlformats.org/wordprocessingml/2006/main}"
MAX_DOCUMENT_XML = 128 << 20
HEADING = re.compile(r"^(?:heading|标题)\s*(\d)$", re.I)


def _parser():
    return etree.XMLParser(resolve_entities=False, no_network=True, load_dtd=False, huge_tree=False)


def _read(z, name, limit):
    info = z.getinfo(name)
    if info.file_size > limit:
        raise ValueError("part too large")
    with z.open(info) as f:
        data = f.read(limit + 1)
    if len(data) > limit:
        raise ValueError("part too large")
    return data


def _styles(z):
    try:
        root = etree.fromstring(_read(z, "word/styles.xml", 16 << 20), _parser())
    except (KeyError, ValueError, etree.XMLSyntaxError):
        return {}
    out = {}
    for st in root.iter(W + "style"):
        sid = st.get(W + "styleId")
        name = st.find(W + "name")
        out[sid] = (name.get(W + "val") if name is not None else sid) or sid
    return out


def _text(p):
    parts = []
    for el in p.iter():
        tag = el.tag
        if tag == W + "t" and not any(a.tag == W + "del" for a in el.iterancestors()):
            parts.append(el.text or "")
        elif tag == W + "tab":
            parts.append("\t")
        elif tag in (W + "br", W + "cr"):
            parts.append("\n")
    return "".join(parts)


def _heading_level(p, styles):
    ppr = p.find(W + "pPr")
    if ppr is None:
        return None, None
    ps = ppr.find(W + "pStyle")
    sid = ps.get(W + "val") if ps is not None else None
    name = styles.get(sid, sid) if sid else None
    level = None
    if name:
        if name.lower() == "title":
            level = 1
        else:
            m = HEADING.match(name.strip())
            if m:
                level = int(m.group(1))
    ol = ppr.find(W + "outlineLvl")
    if level is None and ol is not None:
        try:
            level = int(ol.get(W + "val")) + 1
        except (TypeError, ValueError):
            pass
    return name, level


def docx_facts(path):
    """{"paragraphs": [{"text","style","level"}], "tables": [{"before","rows","cols"}]}"""
    with zipfile.ZipFile(path) as z:
        root = etree.fromstring(_read(z, "word/document.xml", MAX_DOCUMENT_XML), _parser())
        styles = _styles(z)
    body = root.find(W + "body")
    if body is None:
        raise ValueError("no w:body")
    paragraphs, tables = [], []
    for child in body:
        if child.tag == W + "p":
            style, level = _heading_level(child, styles)
            paragraphs.append({"text": _text(child), "style": style, "level": level})
        elif child.tag == W + "tbl":
            rows = child.findall(W + "tr")
            cols = 0
            for tr in rows:
                n = 0
                for tc in tr.findall(W + "tc"):
                    span = tc.find(f"{W}tcPr/{W}gridSpan")
                    n += int(span.get(W + "val")) if span is not None else 1
                cols = max(cols, n)
            tables.append({"before": len(paragraphs), "rows": len(rows), "cols": cols})
    return {"paragraphs": paragraphs, "tables": tables}
