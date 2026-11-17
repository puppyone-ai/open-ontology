#!/usr/bin/env python3
"""Synthetic parser fixtures (B-S3-02, B-S3-03). Stdlib only, deterministic.

    python3 -B testing/super-ontology/fixtures/gen_files.py          # rewrite files/
    python3 -B testing/super-ontology/fixtures/gen_files.py --check  # compare bytes

No clocks, fixed zip timestamps, no real data. Every file is a made-up
document or a hostile container built to trigger one parser decision.
"""
import io
import struct
import sys
import tempfile
import zipfile
from pathlib import Path
from xml.sax.saxutils import escape

OUT = Path(__file__).resolve().parent / "files"
STAMP = (2026, 1, 1, 0, 0, 0)


def col(n):  # 1-based -> letters
    s = ""
    while n:
        n, r = divmod(n - 1, 26)
        s = chr(65 + r) + s
    return s


def zipped(parts, deflate=True):
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        for name, data in parts:
            info = zipfile.ZipInfo(name, STAMP)
            info.compress_type = zipfile.ZIP_DEFLATED if deflate else zipfile.ZIP_STORED
            info.external_attr = 0o644 << 16
            z.writestr(info, data)
    return buf.getvalue()


CT = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>'
    "%s</Types>"
)
STYLES = (
    '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
    '<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main">'
    '<fonts count="1"><font><sz val="11"/><name val="Calibri"/></font></fonts>'
    '<fills count="2"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill></fills>'
    '<borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders>'
    '<cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs>'
    '<cellXfs count="2"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/>'
    '<xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/></cellXfs>'
    "</styleSheet>"
)


def sheet_xml(rows, merges=()):
    """rows: {row_no: [(col_no, spec)]}; spec: ('s', text) | ('n', num) | ('d', serial)
    | ('f', formula, cached-or-None) | ('ss', index) | ('b', bool)"""
    out = ['<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
           '<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>']
    for r in sorted(rows):
        out.append(f'<row r="{r}">')
        for c, spec in sorted(rows[r]):
            ref = f"{col(c)}{r}"
            kind = spec[0]
            if kind == "s":
                out.append(f'<c r="{ref}" t="inlineStr"><is><t>{escape(spec[1])}</t></is></c>')
            elif kind == "ss":
                out.append(f'<c r="{ref}" t="s"><v>{spec[1]}</v></c>')
            elif kind == "n":
                out.append(f'<c r="{ref}"><v>{spec[1]}</v></c>')
            elif kind == "d":
                out.append(f'<c r="{ref}" s="1"><v>{spec[1]}</v></c>')
            elif kind == "b":
                out.append(f'<c r="{ref}" t="b"><v>{int(spec[1])}</v></c>')
            elif kind == "f":
                v = "" if spec[2] is None else f"<v>{spec[2]}</v>"
                out.append(f'<c r="{ref}"><f>{escape(spec[1])}</f>{v}</c>')
        out.append("</row>")
    out.append("</sheetData>")
    if merges:
        out.append(f'<mergeCells count="{len(merges)}">')
        out += [f'<mergeCell ref="{m}"/>' for m in merges]
        out.append("</mergeCells>")
    out.append("</worksheet>")
    return "".join(out)


def xlsx(sheets, shared=(), macros=False):
    """sheets: [(name, rows, merges, state)]"""
    n = len(sheets)
    overrides = "".join(
        f'<Override PartName="/xl/worksheets/sheet{i}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>'
        for i in range(1, n + 1))
    overrides += '<Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/>'
    if shared:
        overrides += '<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>'
    wb = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
          '<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" '
          'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>'
          + "".join(
              f'<sheet name="{escape(nm, {chr(34): "&quot;"})}" sheetId="{i}"'
              + (f' state="{st}"' if st else "") + f' r:id="rId{i}"/>'
              for i, (nm, _, _, st) in enumerate(sheets, 1))
          + "</sheets></workbook>")
    rels = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            + "".join(
                f'<Relationship Id="rId{i}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{i}.xml"/>'
                for i in range(1, n + 1))
            + f'<Relationship Id="rId{n+1}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
            + (f'<Relationship Id="rId{n+2}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/>' if shared else "")
            + "</Relationships>")
    root = ('<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
            '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>'
            "</Relationships>")
    parts = [("[Content_Types].xml", CT % overrides), ("_rels/.rels", root),
             ("xl/workbook.xml", wb), ("xl/_rels/workbook.xml.rels", rels), ("xl/styles.xml", STYLES)]
    if shared:
        parts.append(("xl/sharedStrings.xml",
                      '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>'
                      f'<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{len(shared)}" uniqueCount="{len(shared)}">'
                      + "".join(f"<si><t>{escape(s)}</t></si>" for s in shared) + "</sst>"))
    for i, (_, rows, merges, _) in enumerate(sheets, 1):
        parts.append((f"xl/worksheets/sheet{i}.xml", sheet_xml(rows, merges)))
    if macros:
        parts.append(("xl/vbaProject.bin", b"\xd0\xcf\x11\xe0 synthetic macro stub"))
    return zipped(parts)


def cfb(streams):
    """Minimal OLE2 v3 file; every stream is padded to 4096 bytes (no mini stream)."""
    SECT, END, FAT, FREE, NOSTREAM = 512, 0xFFFFFFFE, 0xFFFFFFFD, 0xFFFFFFFF, 0xFFFFFFFF
    per = 8
    fat = [FAT, END] + [FREE] * 126
    data = b""
    starts = []
    nxt = 2
    for _, payload in streams:
        payload = payload.ljust(4096, b"\0")
        starts.append(nxt)
        for k in range(per):
            fat[nxt + k] = nxt + k + 1 if k < per - 1 else END
        data += payload
        nxt += per
    assert nxt <= 128
    header = (b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1" + b"\0" * 16
              + struct.pack("<HHHHH", 0x3E, 3, 0xFFFE, 9, 6) + b"\0" * 6
              + struct.pack("<IIIIIIIII", 0, 1, 1, 0, 4096, END, 0, END, 0)
              + struct.pack("<I", 0) + struct.pack("<I", FREE) * 108)
    assert len(header) == 512

    def entry(name, typ, left, right, child, start, size):
        raw = name.encode("utf-16-le")
        return (raw.ljust(64, b"\0") + struct.pack("<HBB", len(raw) + 2 if name else 0, typ, 1)
                + struct.pack("<III", left, right, child) + b"\0" * 16 + b"\0" * 4 + b"\0" * 16
                + struct.pack("<IQ", start, size))

    # Directory: root, then the streams chained through right siblings.
    ents = [entry("Root Entry", 5, NOSTREAM, NOSTREAM, 1, END, 0)]
    for i, ((name, payload), start) in enumerate(zip(streams, starts)):
        right = i + 2 if i + 1 < len(streams) else NOSTREAM
        ents.append(entry(name, 2, NOSTREAM, right, NOSTREAM, start, 4096))
    dirsec = b"".join(ents).ljust(512, b"\0")
    assert len(dirsec) == 512
    return header + struct.pack("<128I", *fat) + dirsec + data


def tiny_pdf(encrypted):
    body = [b"%PDF-1.4\n"]
    body.append(b"1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n")
    body.append(b"2 0 obj\n<< /Type /Pages /Kids [3 0 R] /Count 1 >>\nendobj\n")
    body.append(b"3 0 obj\n<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] >>\nendobj\n")
    if encrypted:
        body.append(b"4 0 obj\n<< /Filter /Standard /V 1 /R 2 /O (aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa) "
                    b"/U (bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb) /P -4 >>\nendobj\n")
        trailer = b"trailer\n<< /Size 5 /Root 1 0 R /Encrypt 4 0 R >>\nstartxref\n0\n%%EOF\n"
    else:
        trailer = b"trailer\n<< /Size 4 /Root 1 0 R >>\nstartxref\n0\n%%EOF\n"
    return b"".join(body) + trailer


def zeros_entry(name, size):
    """A deflated entry of `size` zero bytes written in chunks."""
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w") as z:
        z.writestr(zipfile.ZipInfo("[Content_Types].xml", STAMP), CT % "")
        info = zipfile.ZipInfo(name, STAMP)
        info.compress_type = zipfile.ZIP_DEFLATED
        with z.open(info, "w", force_zip64=True) as f:
            chunk = b"\0" * (1 << 20)
            left = size
            while left:
                n = min(left, len(chunk))
                f.write(chunk[:n])
                left -= n
    return buf.getvalue()


def build():
    f = {}
    # --- xlsx ---------------------------------------------------------------
    rows = {
        1: [(1, ("s", "2026 预算"))],
        3: [(1, ("s", "姓名")), (2, ("s", "部门")), (3, ("s", "金额")), (4, ("s", "入职日期"))],
        4: [(1, ("ss", 0)), (2, ("s", "研发")), (3, ("n", 1200)), (4, ("d", 46082))],
        5: [(1, ("ss", 1)), (3, ("n", 980.5)), (4, ("d", 46100.75))],
        6: [(1, ("s", "合计")), (3, ("f", "SUM(C4:C5)", 2180.5))],
        7: [(1, ("s", "待算")), (3, ("f", "C6*2", None))],
        9: [(1, ("s", "编号")), (2, ("s", "状态"))],
        10: [(1, ("n", 101)), (2, ("b", True))],
    }
    f["xlsx-merged-regions.xlsx"] = xlsx(
        [("预算 2026", rows, ["B4:B5"], None)], shared=["林远舟", "周若溪"])
    f["xlsx-hidden-sheet.xlsx"] = xlsx([
        ("公开", {1: [(1, ("s", "项")), (2, ("s", "值"))], 2: [(1, ("s", "a")), (2, ("n", 1))]}, [], None),
        ("内部", {1: [(1, ("s", "secret")), (2, ("s", "x"))], 2: [(1, ("s", "b")), (2, ("n", 2))]}, [], "hidden"),
    ])
    f["xlsx-sparse-corners.xlsx"] = xlsx([
        ("S", {1: [(1, ("s", "top"))], 1048576: [(16384, ("s", "bottom"))]}, [], None)])
    f["xlsx-macros.xlsx"] = xlsx([("S", {1: [(1, ("s", "h1")), (2, ("s", "h2"))], 2: [(1, ("s", "a")), (2, ("s", "b"))]}, [], None)], macros=True)
    f["xlsx-empty.xlsx"] = xlsx([("S", {}, [], None)])
    # --- hostile containers --------------------------------------------------
    f["zip-bomb-size.xlsx"] = zeros_entry("xl/worksheets/sheet1.xml", 260 << 20)
    f["zip-bomb-ratio.docx"] = zeros_entry("word/document.xml", 8 << 20)
    f["zip-traversal.docx"] = zipped([("[Content_Types].xml", CT % ""), ("../evil.xml", b"x")])
    f["zip-plain.zip"] = zipped([("notes.txt", b"hello")])
    f["docx-encrypted.docx"] = cfb([("EncryptionInfo", b"\x04\x00\x04\x00synthetic"),
                                    ("EncryptedPackage", b"\x00" * 8 + b"synthetic")])
    f["legacy.doc"] = cfb([("WordDocument", b"\xec\xa5\xc1\x00synthetic")])
    f["pdf-encrypted.pdf"] = tiny_pdf(True)
    f["pdf-plain.pdf"] = tiny_pdf(False)
    # --- text ----------------------------------------------------------------
    f["gb18030.txt"] = "你好，世界。这是一份 GB18030 编码的文本。\n".encode("gb18030")
    f["gb18030.csv"] = "姓名,部门\n林远舟,管理\n".encode("gb18030")
    f["empty.csv"] = b""
    f["comma.csv"] = "编号,任务,负责人\nT-1,整理 OKR,周若溪\nT-2,\"开通 \"\"北辰\"\" 环境\",陈子墨\n".encode()
    f["semicolon.csv"] = "id;name;note\n1;a;x\n2;b;y\n3;c;z\n".encode()
    f["tab.csv"] = "id\tname\n1\ta,b\n2\tc,d\n".encode()
    f["quoted-multiline.csv"] = 'id,note\n1,"line one\nline two"\n2,plain\n'.encode()
    f["blank-lines.csv"] = "a,b\n1,2\n\n3,4\n".encode()
    f["no-header.csv"] = "1,2,3\n4,5,6\n7,8,9\n".encode()
    f["utf16-bom.csv"] = b"\xff\xfe" + "k,v\n1,2\n".encode("utf-16-le")
    return f


def main():
    files = build()
    if "--check" in sys.argv:
        bad = [n for n, b in files.items() if not (OUT / n).exists() or (OUT / n).read_bytes() != b]
        extra = [p.name for p in OUT.iterdir() if p.name not in files] if OUT.exists() else []
        if bad or extra:
            print("stale:", bad, "extra:", extra)
            sys.exit(1)
        print(f"ok: {len(files)} files match")
        return
    OUT.mkdir(parents=True, exist_ok=True)
    for n, b in files.items():
        (OUT / n).write_bytes(b)
    print(f"wrote {len(files)} files to {OUT}")


if __name__ == "__main__":
    main()
