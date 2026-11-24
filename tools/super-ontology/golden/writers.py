"""Deterministic writers for md/txt/csv/xlsx/docx/pptx/pdf (stdlib only).

Every writer returns (bytes, infos) where infos is a list of dicts
{"tag", "text", "anchor"} in block order. Zip containers use ZIP_STORED with a fixed
timestamp so the bytes do not depend on the zlib version.
"""
import io
import zipfile
from xml.sax.saxutils import escape

STAMP = (2026, 1, 1, 0, 0, 0)
CREATOR = "青岚智造综合办"


def zip_bytes(parts):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_STORED) as z:
        for name, data in parts:
            zi = zipfile.ZipInfo(name, STAMP)
            zi.compress_type = zipfile.ZIP_STORED
            zi.create_system = 3
            zi.external_attr = 0o644 << 16
            z.writestr(zi, data.encode("utf-8") if isinstance(data, str) else data)
    return buf.getvalue()


def esc(s):
    return escape(str(s), {'"': "&quot;"})


def flat(b):
    k = b[0]
    if k in ("trow", "row"):
        return " ".join(str(c) for c in b[1])
    if k == "slide":
        return " ".join([b[1]] + list(b[2]))
    return b[1]


def tag_of(b):
    return b[3] if b[0] == "slide" else b[2]


def info(b, anchor):
    return {"tag": tag_of(b), "text": flat(b), "anchor": anchor}


# ------------------------------------------------------------------ text-like
def write_md(d):
    lines, infos = [], []
    if d["fm"]:
        lines.append("---")
        lines += [f"{k}: {v}" for k, v in d["fm"].items()]
        lines += ["---", ""]
    prev = None
    for b in d["blocks"]:
        k = b[0]
        if lines and lines[-1] != "" and not (k == "li" and prev == "li"):
            lines.append("")
        txt = {"h1": "# ", "h2": "## ", "li": "- "}.get(k, "") + b[1]
        lines.append(txt)
        n = len(lines)
        infos.append(info(b, {"frag": f"#L{n}-L{n}"}))
        prev = k
    return ("\n".join(lines) + "\n").encode("utf-8"), infos


def write_txt(d):
    lines = [b[1] for b in d["blocks"]]
    infos = [info(b, {"frag": f"#L{i}-L{i}"}) for i, b in enumerate(d["blocks"], 1)]
    return ("\n".join(lines) + "\n").encode("utf-8"), infos


def write_csv(d):
    lines, infos = [], []
    for i, b in enumerate(d["blocks"], 1):
        assert not any("," in str(c) or '"' in str(c) for c in b[1])
        lines.append(",".join(str(c) for c in b[1]))
        infos.append(info(b, {"frag": f"#R{i}"}))
    return ("\n".join(lines) + "\n").encode("utf-8"), infos


# ------------------------------------------------------------------ OOXML common
CT_REL = "application/vnd.openxmlformats-package.relationships+xml"
NS_REL = "http://schemas.openxmlformats.org/package/2006/relationships"
RT = "http://schemas.openxmlformats.org/officeDocument/2006/relationships"
XML = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'


def content_types(overrides):
    o = "".join(f'<Override PartName="{p}" ContentType="{c}"/>' for p, c in overrides)
    return (XML + '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
            f'<Default Extension="rels" ContentType="{CT_REL}"/><Default Extension="xml" ContentType="application/xml"/>{o}</Types>')


def rels(items):
    r = "".join(f'<Relationship Id="{i}" Type="{t}" Target="{g}"/>' for i, t, g in items)
    return XML + f'<Relationships xmlns="{NS_REL}">{r}</Relationships>'


def core(d):
    return (XML + '<cp:coreProperties xmlns:cp="http://schemas.openxmlformats.org/package/2006/metadata/core-properties" '
            'xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:dcterms="http://purl.org/dc/terms/" '
            'xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">'
            f'<dc:title>{esc(d["title"])}</dc:title><dc:creator>{CREATOR}</dc:creator>'
            f'<dcterms:created xsi:type="dcterms:W3CDTF">{d["created"]}T00:00:00Z</dcterms:created>'
            f'<dcterms:modified xsi:type="dcterms:W3CDTF">{d["created"]}T00:00:00Z</dcterms:modified>'
            '</cp:coreProperties>')


CORE_REL = ("rId9", "http://schemas.openxmlformats.org/package/2006/relationships/metadata/core-properties", "docProps/core.xml")
CORE_CT = ("/docProps/core.xml", "application/vnd.openxmlformats-package.core-properties+xml")


# ------------------------------------------------------------------ docx
def write_docx(d):
    W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
    body, infos = [], []
    para = 0
    tbl = 0
    rows = []  # pending table rows
    row_infos = []

    def flush():
        nonlocal tbl, rows, row_infos
        if not rows:
            return
        tbl += 1
        ncol = max(len(r) for r in rows)
        grid = "".join('<w:gridCol w:w="2800"/>' for _ in range(ncol))
        trs = []
        for r in rows:
            tcs = "".join(f'<w:tc><w:tcPr><w:tcW w:w="2800" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>{esc(c)}</w:t></w:r></w:p></w:tc>' for c in r)
            trs.append(f"<w:tr>{tcs}</w:tr>")
        body.append('<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>'
                    f'<w:tblGrid>{grid}</w:tblGrid>{"".join(trs)}</w:tbl>')
        for ri, (b, n) in enumerate(row_infos, 1):
            infos.append(info(b, {"frag": f"#T{tbl}!R{ri}C1:R{ri}C{n}"}))
        rows, row_infos = [], []

    for b in d["blocks"]:
        k = b[0]
        if k == "trow":
            rows.append(b[1])
            row_infos.append((b, len(b[1])))
            continue
        flush()
        style = {"h1": "Heading1", "h2": "Heading2"}.get(k)
        ppr = f'<w:pPr><w:pStyle w:val="{style}"/></w:pPr>' if style else ""
        body.append(f'<w:p>{ppr}<w:r><w:t xml:space="preserve">{esc(b[1])}</w:t></w:r></w:p>')
        infos.append(info(b, {"frag": f"#¶{para}-{para}"}))
        para += 1
    flush()
    # a docx table must be followed by a paragraph before sectPr
    body.append("<w:p/>")
    doc = (XML + f'<w:document {W}><w:body>{"".join(body)}'
           '<w:sectPr><w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/></w:sectPr></w:body></w:document>')
    styles = (XML + f'<w:styles {W}><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="SimSun"/><w:sz w:val="22"/></w:rPr></w:rPrDefault></w:docDefaults>'
              '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/></w:style>'
              '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:b/><w:sz w:val="36"/></w:rPr></w:style>'
              '<w:style w:type="paragraph" w:styleId="Heading2"><w:name w:val="heading 2"/><w:basedOn w:val="Normal"/><w:pPr><w:keepNext/><w:outlineLvl w:val="1"/></w:pPr><w:rPr><w:b/><w:sz w:val="28"/></w:rPr></w:style>'
              '<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:tblPr><w:tblBorders>'
              + "".join(f'<w:{s} w:val="single" w:sz="4" w:space="0" w:color="808080"/>' for s in ("top", "left", "bottom", "right", "insideH", "insideV"))
              + '</w:tblBorders></w:tblPr></w:style></w:styles>')
    parts = [
        ("[Content_Types].xml", content_types([("/word/document.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"),
                                               ("/word/styles.xml", "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"), CORE_CT])),
        ("_rels/.rels", rels([("rId1", RT + "/officeDocument", "word/document.xml"), CORE_REL])),
        ("word/_rels/document.xml.rels", rels([("rId1", RT + "/styles", "styles.xml")])),
        ("word/document.xml", doc), ("word/styles.xml", styles), ("docProps/core.xml", core(d)),
    ]
    return zip_bytes(parts), infos


# ------------------------------------------------------------------ xlsx
def col(i):
    s = ""
    i += 1
    while i:
        i, r = divmod(i - 1, 26)
        s = chr(65 + r) + s
    return s


def write_xlsx(d):
    rows, infos = [], []
    for r, b in enumerate(d["blocks"], 1):
        cells = []
        for c, v in enumerate(b[1]):
            ref = f"{col(c)}{r}"
            if isinstance(v, int):
                cells.append(f'<c r="{ref}"><v>{v}</v></c>')
            else:
                cells.append(f'<c r="{ref}" t="inlineStr"><is><t>{esc(v)}</t></is></c>')
        rows.append(f'<row r="{r}">{"".join(cells)}</row>')
        sheet = d["title"]
        infos.append(info(b, {"frag": f"#'{sheet}'!A{r}:{col(len(b[1]) - 1)}{r}"}))
    S = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
    parts = [
        ("[Content_Types].xml", content_types([("/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"),
                                               ("/xl/worksheets/sheet1.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"), CORE_CT])),
        ("_rels/.rels", rels([("rId1", RT + "/officeDocument", "xl/workbook.xml"), CORE_REL])),
        ("xl/workbook.xml", XML + f'<workbook {S} xmlns:r="{RT}"><sheets><sheet name="{esc(d["title"])}" sheetId="1" r:id="rId1"/></sheets></workbook>'),
        ("xl/_rels/workbook.xml.rels", rels([("rId1", RT + "/worksheet", "worksheets/sheet1.xml")])),
        ("xl/worksheets/sheet1.xml", XML + f'<worksheet {S}><sheetData>{"".join(rows)}</sheetData></worksheet>'),
        ("docProps/core.xml", core(d)),
    ]
    return zip_bytes(parts), infos


# ------------------------------------------------------------------ pptx
THEME = (XML + '<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Plain"><a:themeElements>'
         '<a:clrScheme name="Plain">' + "".join(f'<a:{n}><a:srgbClr val="{v}"/></a:{n}>' for n, v in
         [("dk1", "000000"), ("lt1", "FFFFFF"), ("dk2", "333333"), ("lt2", "EEEEEE"), ("accent1", "3366CC"), ("accent2", "CC6633"),
          ("accent3", "669933"), ("accent4", "996699"), ("accent5", "339999"), ("accent6", "CC9933"), ("hlink", "0000FF"), ("folHlink", "800080")])
         + '</a:clrScheme><a:fontScheme name="Plain"><a:majorFont><a:latin typeface="Calibri"/><a:ea typeface="SimSun"/><a:cs typeface=""/></a:majorFont>'
         '<a:minorFont><a:latin typeface="Calibri"/><a:ea typeface="SimSun"/><a:cs typeface=""/></a:minorFont></a:fontScheme>'
         '<a:fmtScheme name="Plain"><a:fillStyleLst>' + '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' * 3 + '</a:fillStyleLst>'
         '<a:lnStyleLst>' + '<a:ln w="9525"><a:solidFill><a:schemeClr val="phClr"/></a:solidFill></a:ln>' * 3 + '</a:lnStyleLst>'
         '<a:effectStyleLst>' + '<a:effectStyle><a:effectLst/></a:effectStyle>' * 3 + '</a:effectStyleLst>'
         '<a:bgFillStyleLst>' + '<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>' * 3 + '</a:bgFillStyleLst></a:fmtScheme>'
         '</a:themeElements></a:theme>')
PNS = ('xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" '
       f'xmlns:r="{RT}" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"')


EMPTY_P = '<a:p><a:endParaRPr lang="zh-CN"/></a:p>'


def ph(id_, name, phattr, x, y, cx, cy, paras=""):
    paras = paras or EMPTY_P
    return (f'<p:sp><p:nvSpPr><p:cNvPr id="{id_}" name="{name}"/><p:cNvSpPr><a:spLocks noGrp="1"/></p:cNvSpPr><p:nvPr><p:ph {phattr}/></p:nvPr></p:nvSpPr>'
            f'<p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm></p:spPr>'
            f'<p:txBody><a:bodyPr/><a:lstStyle/>{paras}</p:txBody></p:sp>')


GRP = ('<p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr>'
       '<p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>')


def write_pptx(d):
    slides = [b for b in d["blocks"] if b[0] == "slide"]
    infos = [info(b, {"frag": f"#s{i}"}) for i, b in enumerate(slides, 1)]
    master = (XML + f'<p:sldMaster {PNS}><p:cSld><p:spTree>{GRP}'
              + ph(2, "Title", 'type="title"', 457200, 274638, 8229600, 1143000)
              + ph(3, "Body", 'type="body" idx="1"', 457200, 1600200, 8229600, 4525963)
              + '</p:spTree></p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/>'
              '<p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>')
    layout = (XML + f'<p:sldLayout {PNS} type="obj"><p:cSld name="Title and Content"><p:spTree>{GRP}'
              + ph(2, "Title", 'type="title"', 457200, 274638, 8229600, 1143000)
              + ph(3, "Content", 'idx="1"', 457200, 1600200, 8229600, 4525963)
              + '</p:spTree></p:cSld></p:sldLayout>')
    parts = [
        ("[Content_Types].xml", content_types(
            [("/ppt/presentation.xml", "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"),
             ("/ppt/slideMasters/slideMaster1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"),
             ("/ppt/slideLayouts/slideLayout1.xml", "application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"),
             ("/ppt/theme/theme1.xml", "application/vnd.openxmlformats-officedocument.theme+xml"), CORE_CT]
            + [(f"/ppt/slides/slide{i}.xml", "application/vnd.openxmlformats-officedocument.presentationml.slide+xml") for i in range(1, len(slides) + 1)])),
        ("_rels/.rels", rels([("rId1", RT + "/officeDocument", "ppt/presentation.xml"), CORE_REL])),
        ("ppt/presentation.xml", XML + f'<p:presentation {PNS}><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst>'
         '<p:sldIdLst>' + "".join(f'<p:sldId id="{255 + i}" r:id="rId{i + 1}"/>' for i in range(1, len(slides) + 1)) + '</p:sldIdLst>'
         '<p:sldSz cx="9144000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>'),
        ("ppt/_rels/presentation.xml.rels", rels([("rId1", RT + "/slideMaster", "slideMasters/slideMaster1.xml")]
                                                 + [(f"rId{i + 1}", RT + "/slide", f"slides/slide{i}.xml") for i in range(1, len(slides) + 1)]
                                                 + [(f"rId{len(slides) + 2}", RT + "/theme", "theme/theme1.xml")])),
        ("ppt/slideMasters/slideMaster1.xml", master),
        ("ppt/slideMasters/_rels/slideMaster1.xml.rels", rels([("rId1", RT + "/slideLayout", "../slideLayouts/slideLayout1.xml"), ("rId2", RT + "/theme", "../theme/theme1.xml")])),
        ("ppt/slideLayouts/slideLayout1.xml", layout),
        ("ppt/slideLayouts/_rels/slideLayout1.xml.rels", rels([("rId1", RT + "/slideMaster", "../slideMasters/slideMaster1.xml")])),
        ("ppt/theme/theme1.xml", THEME), ("docProps/core.xml", core(d)),
    ]
    for i, b in enumerate(slides, 1):
        title = f'<a:p><a:r><a:rPr lang="zh-CN"/><a:t>{esc(b[1])}</a:t></a:r></a:p>'
        bullets = "".join(f'<a:p><a:r><a:rPr lang="zh-CN"/><a:t>{esc(t)}</a:t></a:r></a:p>' for t in b[2]) or EMPTY_P
        xml = (XML + f'<p:sld {PNS}><p:cSld><p:spTree>{GRP}'
               + ph(2, "Title", 'type="title"', 457200, 274638, 8229600, 1143000, title)
               + ph(3, "Content", 'idx="1"', 457200, 1600200, 8229600, 4525963, bullets) + '</p:spTree></p:cSld></p:sld>')
        parts.append((f"ppt/slides/slide{i}.xml", xml))
        parts.append((f"ppt/slides/_rels/slide{i}.xml.rels", rels([("rId1", RT + "/slideLayout", "../slideLayouts/slideLayout1.xml")])))
    return zip_bytes(parts), infos


# ------------------------------------------------------------------ pdf
PW, PH, MX, MTOP, MBOT = 595, 842, 72, 72, 72
MAXW = PW - 2 * MX


def cw(c, size):
    o = ord(c)
    return size if (o >= 0x2E80 or 0xFF00 <= o <= 0xFFEF or 0x3000 <= o <= 0x303F or 0x2018 <= o <= 0x201F) else size * 0.5


def wrap(text, size):
    lines, cur, w = [], "", 0.0
    for c in text:
        if w + cw(c, size) > MAXW and cur:
            lines.append((cur, w))
            cur, w = "", 0.0
        cur += c
        w += cw(c, size)
    lines.append((cur, w))
    return lines


def write_pdf(d):
    pages = [[]]
    y = PH - MTOP
    infos = []
    for b in d["blocks"]:
        size, lead = (16, 26) if b[0] == "h1" else ((14, 22) if b[0] == "h2" else (12, 20))
        lines = wrap(b[1], size)
        need = len(lines) * lead
        if y - need < MBOT:
            pages.append([])
            y = PH - MTOP
        x1 = 0.0
        top = bot = None
        for text, w in lines:
            y -= lead
            pages[-1].append((y, size, text))
            x1 = max(x1, w)
            t, bt = y + size * 0.88, y - size * 0.12
            top = t if top is None else top
            bot = bt
        y -= 6
        bbox = [round(MX * 100), round((PH - top) * 100), round((MX + x1) * 100), round((PH - bot) * 100)]
        infos.append(info(b, {"frag": f"#p{len(pages)}", "bbox": bbox}))
    objs = {}
    nump = len(pages)
    first = 7
    kids = " ".join(f"{first + 2 * i} 0 R" for i in range(nump))
    objs[1] = b"<< /Type /Catalog /Pages 2 0 R >>"
    objs[2] = f"<< /Type /Pages /Kids [{kids}] /Count {nump} >>".encode()
    objs[3] = b"<< /Type /Font /Subtype /Type0 /BaseFont /STSong-Light /Encoding /UniGB-UCS2-H /DescendantFonts [4 0 R] /ToUnicode 6 0 R >>"
    objs[4] = (b"<< /Type /Font /Subtype /CIDFontType0 /BaseFont /STSong-Light /CIDSystemInfo << /Registry (Adobe) /Ordering (GB1) /Supplement 2 >> "
               b"/FontDescriptor 5 0 R /DW 1000 >>")
    objs[5] = (b"<< /Type /FontDescriptor /FontName /STSong-Light /Flags 6 /FontBBox [-25 -254 1000 880] /ItalicAngle 0 "
               b"/Ascent 880 /Descent -120 /CapHeight 880 /StemV 93 >>")
    cmap = (b"/CIDInit /ProcSet findresource begin 12 dict begin begincmap /CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def "
            b"/CMapName /Adobe-Identity-UCS def /CMapType 2 def 1 begincodespacerange <0000> <FFFF> endcodespacerange "
            b"1 beginbfrange <0000> <FFFF> <0000> endbfrange endcmap CMapName currentdict /CMap defineresource pop end end")
    objs[6] = b"<< /Length %d >>\nstream\n" % len(cmap) + cmap + b"\nendstream"
    for i, pg in enumerate(pages):
        ops = ["BT"]
        for (yy, size, text) in pg:
            hexs = "".join("%04X" % ord(c) for c in text)
            ops.append(f"/F1 {size} Tf 1 0 0 1 {MX} {yy:.2f} Tm <{hexs}> Tj")
        ops.append("ET")
        stream = "\n".join(ops).encode("ascii")
        objs[first + 2 * i] = (f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {PW} {PH}] /Resources << /Font << /F1 3 0 R >> >> /Contents {first + 2 * i + 1} 0 R >>").encode()
        objs[first + 2 * i + 1] = b"<< /Length %d >>\nstream\n" % len(stream) + stream + b"\nendstream"
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offs = {}
    for n in sorted(objs):
        offs[n] = len(out)
        out += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    xref = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1)
    for n in sorted(objs):
        out += b"%010d 00000 n \n" % offs[n]
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, xref)
    return bytes(out), infos


WRITERS = {"md": write_md, "txt": write_txt, "csv": write_csv, "xlsx": write_xlsx,
           "docx": write_docx, "pptx": write_pptx, "pdf": write_pdf}
