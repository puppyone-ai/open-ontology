#!/usr/bin/env python3
"""Deterministic generator of the acceptance folder, its gold files and MANIFEST.

    python3 -B tools/super-ontology/acceptance/gen.py [--out DIR] [--check]

Stdlib only, plus macOS `say`, `afconvert`, `ffmpeg` (audio) and `swiftc` (images, scans). Seeded: no clocks, no random.
Writes <out>/project/ (the folder to drop), <out>/gold/{relations,unlinked,questions}.jsonl and MANIFEST.sha256.
All content is synthetic (see accworld.py). The gold files are frozen before any run (see README.md).
"""
import argparse, hashlib, json, os, shutil, struct, subprocess, sys, tempfile, zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.append(str(HERE.parent / "golden"))
import writers as W  # noqa: E402
from accworld import *  # noqa: E402,F401

TEXT = {}   # relative path -> source text (what a reader/listener can get from the file); used to check the gold
FILES = {}  # relative path -> bytes
fmt = lambda n: f"{n:,}"
P = lambda k: PEOPLE[k]


def put(path, data, text=None):
    FILES[path] = data if isinstance(data, bytes) else data.encode("utf-8")
    TEXT[path] = text if text is not None else (data if isinstance(data, str) else "")


def doc(title, blocks, fm=None):
    blocks = [b if (b[0] == "slide" and len(b) == 4) or (b[0] != "slide" and len(b) == 3) else b + (None,) for b in blocks]
    return {"title": title, "blocks": blocks, "fm": fm or {}, "created": "2027-01-12"}


def flat_text(d):
    return "\n".join(W.flat(b) for b in d["blocks"])


def narrow_latin(pdf):
    """The golden writer's CID font has one width for every glyph (full width), which is right for Chinese but spreads Latin text far past
    the page edge. Adobe-GB1 CIDs 1..95 are the proportional ASCII glyphs; give them half width and rebuild the cross-reference table."""
    import re
    body = pdf[:pdf.index(b"xref\n0 ")].replace(b"/DW 1000 >>", b"/DW 1000 /W [ 1 95 500 ] >>")
    offs = {int(m.group(1)): m.start() + 1 for m in re.finditer(rb"\n(\d+) 0 obj\n", body)}
    out = bytearray(body)
    x = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(offs) + 1) + b"".join(b"%010d 00000 n \n" % offs[n] for n in sorted(offs))
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(offs) + 1, x)
    return bytes(out)


def put_doc(path, d, kind):
    data, _ = W.WRITERS[kind](d)
    if kind == "pdf":
        data = narrow_latin(data)
    put(path, data, flat_text(d))


def name2(k):
    return f"{P(k)[0]} ({P(k)[1]})"


# ------------------------------------------------------------------ xlsx with several sheets
def xlsx_multi(sheets, title):
    S = 'xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"'
    ct = [("/xl/workbook.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"), W.CORE_CT]
    ct += [(f"/xl/worksheets/sheet{i}.xml", "application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml") for i in range(1, len(sheets) + 1)]
    parts = [("[Content_Types].xml", W.content_types(ct)),
             ("_rels/.rels", W.rels([("rId1", W.RT + "/officeDocument", "xl/workbook.xml"), W.CORE_REL])),
             ("xl/workbook.xml", W.XML + f'<workbook {S} xmlns:r="{W.RT}"><sheets>' + "".join(
                 f'<sheet name="{W.esc(n)}" sheetId="{i}" r:id="rId{i}"/>' for i, (n, _) in enumerate(sheets, 1)) + "</sheets></workbook>"),
             ("xl/_rels/workbook.xml.rels", W.rels([(f"rId{i}", W.RT + "/worksheet", f"worksheets/sheet{i}.xml") for i in range(1, len(sheets) + 1)]))]
    for i, (n, rows) in enumerate(sheets, 1):
        xr = []
        for r, row in enumerate(rows, 1):
            cs = []
            for c, v in enumerate(row):
                ref = f"{W.col(c)}{r}"
                cs.append(f'<c r="{ref}"><v>{v}</v></c>' if isinstance(v, int) else f'<c r="{ref}" t="inlineStr"><is><t>{W.esc(v)}</t></is></c>')
            xr.append(f'<row r="{r}">{"".join(cs)}</row>')
        parts.append((f"xl/worksheets/sheet{i}.xml", W.XML + f'<worksheet {S}><sheetData>{"".join(xr)}</sheetData></worksheet>'))
    parts.append(("docProps/core.xml", W.core({"title": title, "created": "2027-01-12"})))
    return W.zip_bytes(parts)


def put_xlsx(path, sheets, title):
    put(path, xlsx_multi(sheets, title), "\n".join(f"[{n}] " + " | ".join(str(c) for c in r) for n, rows in sheets for r in rows))


def csv_text(rows):
    return "\n".join(",".join(str(c) for c in r) for r in rows) + "\n"


# ------------------------------------------------------------------ images, scans, audio
def render(jobs, tmp):
    spec = Path(tmp) / "jobs.json"
    spec.write_text(json.dumps(jobs), encoding="utf-8")
    exe = Path(tmp) / "render"
    if not exe.exists():
        subprocess.run(["swiftc", "-O", str(HERE / "render.swift"), "-o", str(exe)], check=True, capture_output=True)
    subprocess.run([str(exe), str(spec)], check=True)


def scanned_pdf(gray, w, h):
    """Image-only PDF: one Flate gray image per page, no text, no fonts, no metadata."""
    pw, ph = 595, 842
    img = zlib.compress(gray, 9)
    objs = {1: b"<< /Type /Catalog /Pages 2 0 R >>", 2: b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            3: f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw} {ph}] /Resources << /XObject << /Im0 4 0 R >> >> /Contents 5 0 R >>".encode(),
            4: f"<< /Type /XObject /Subtype /Image /Width {w} /Height {h} /ColorSpace /DeviceGray /BitsPerComponent 8 /Filter /FlateDecode /Length {len(img)} >>\nstream\n".encode() + img + b"\nendstream"}
    content = f"q {pw} 0 0 {ph} 0 0 cm /Im0 Do Q".encode()
    objs[5] = b"<< /Length %d >>\nstream\n" % len(content) + content + b"\nendstream"
    out = bytearray(b"%PDF-1.4\n%\xe2\xe3\xcf\xd3\n")
    offs = {}
    for n in sorted(objs):
        offs[n] = len(out)
        out += b"%d 0 obj\n" % n + objs[n] + b"\nendobj\n"
    x = len(out)
    out += b"xref\n0 %d\n0000000000 65535 f \n" % (len(objs) + 1) + b"".join(b"%010d 00000 n \n" % offs[n] for n in sorted(objs))
    out += b"trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (len(objs) + 1, x)
    return bytes(out)


def say_mix(segments, out_path, tmp, kind):
    """segments: [(voice, rate_wpm, text)] -> one file (wav 16 kHz mono or m4a AAC)."""
    parts = []
    for i, (voice, rate, text) in enumerate(segments):
        aiff = Path(tmp) / f"seg{i}.aiff"
        subprocess.run(["say", "-v", voice, "-r", str(rate), "-o", str(aiff), text], check=True)
        parts.append(aiff)
    lst = Path(tmp) / "list.txt"
    lst.write_text("".join(f"file '{p}'\n" for p in parts))
    wav = Path(tmp) / "mix.wav"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), "-ar", "16000", "-ac", "1",
                    "-fflags", "+bitexact", "-flags:a", "+bitexact", str(wav)], check=True)
    if kind == "wav":
        return wav.read_bytes()
    m4a = Path(tmp) / "mix.m4a"
    subprocess.run(["ffmpeg", "-y", "-loglevel", "error", "-i", str(wav), "-c:a", "aac", "-b:a", "48k", "-fflags", "+bitexact",
                    "-flags:a", "+bitexact", "-map_metadata", "-1", str(m4a)], check=True)
    return m4a.read_bytes()


def duration(path):
    r = subprocess.run(["ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path)], capture_output=True, text=True)
    return float(r.stdout.strip() or 0)


# ================================================================== content
def build(tmp):
    gu, bai, ortiz, shao, raman, jiang, cole, ke, mao = (P(k) for k in ("gu", "bai", "ortiz", "shao", "raman", "jiang", "cole", "ke", "mao"))
    # ---------------- people
    rows = [["姓名 Name", "English name", "角色 Role", "Role (EN)", "团队 Team", "Email"]]
    rows += [[v[0], v[1], v[2], v[3], v[4], v[5]] for v in PEOPLE.values()]
    put_xlsx("people/team-roster.xlsx", [("Roster", rows), ("Teams", [["Team", "Lead"], ["Product", gu[0]], ["Program Office", bai[0]], ["Engineering", ortiz[1]],
             ["Finance", shao[0]], ["Marketing", raman[1]], ["Operations", jiang[0]], ["Legal", cole[1]], ["Quality", ke[0]], ["Design", mao[0]]])], "Northport Optics roster")
    put("people/contact-directory.csv", csv_text([["name", "email", "office"]] + [[v[1], v[5], o] for v, o in zip(PEOPLE.values(),
        ["Shanghai", "Shanghai", "Austin", "Shanghai", "Austin", "Shenzhen", "Austin", "Shenzhen", "Shanghai"])]))
    # ---------------- finance
    summ = [["Category", "Budget CNY", "Owner"]] + [[k, SUM[k], o] for k, o in zip(BUDGET, [ortiz[1], raman[1], jiang[0], jiang[0], ke[0], shao[0]])] + [["TOTAL", TOTAL, shao[0]]]
    sheets = [("Summary", summ)]
    for cat in ("Marketing", "Hardware", "Logistics"):
        sheets.append((cat, [["Line", "Budget CNY"]] + [[k, v] for k, v in BUDGET[cat].items()] + [["Subtotal", SUM[cat]]]))
    sheets.append(("Changes", [["Date", "From", "To", "Amount CNY", "Approved by"], ["2027-01-26", "Print", "Video production", 80000, gu[1]]]))
    put_xlsx("finance/launch-budget-2027.xlsx", sheets, "Aurora NP-7 launch budget 2027")
    vrows = [["Vendor", "供应商", "Category", "Internal owner", "Contract CNY", "Status"]] + [
        [v[1], v[0], v[2], P(v[3])[1], v[4], v[5]] for v in VENDORS.values()]
    put_xlsx("finance/vendor-list.xlsx", [("Vendors", vrows), ("Contacts", [["Vendor", "Contact"], ["Hengding Optoelectronics", "Mr. Lu Fan"],
             ["Meridian Packaging", "Ms. Ines Duarte"], ["Suda Express", "Mr. Kai Wen"], ["Yunfan Media", "Ms. Bao Yi"]])], "Vendor list")
    put("finance/actuals-2027-q1.csv", csv_text([["month", "category", "vendor", "amount_cny"],
        ["2027-01", "Hardware", "Hengding Optoelectronics", 744000], ["2027-01", "Marketing", "Yunfan Media", 210000],
        ["2027-02", "Hardware", "Hengding Optoelectronics", 744000], ["2027-02", "Marketing", "Yunfan Media", 330000],
        ["2027-02", "Logistics", "Suda Express", 196000], ["2027-02", "Certification and QA", "Northport QA lab", 120000]]))
    put("finance/po-log.csv", csv_text([["po", "vendor", "item", "amount_cny", "approver", "date"],
        ["PO-1001", "Hengding Optoelectronics", "Lens deposit 30 percent", 744000, gu[1], "2027-01-21"],
        ["PO-1002", "Yunfan Media", "Launch video production", 580000, raman[1], "2027-01-27"],
        ["PO-1003", "Suda Express", "Air freight block", 520000, jiang[1], "2027-02-03"],
        ["PO-1004", "Meridian Packaging", "Retail box first run", 240000, raman[1], "2027-02-15"]]))
    # ---------------- PDFs
    contract = doc("Lens Supply Agreement 镜片供货合同", [
        ("h1", "Lens Supply Agreement 镜片供货合同"),
        ("p", f"Contract no. NPO-2027-014. Signed 2027-01-20 between {COMPANY[1]} ({COMPANY[0]}) and {VENDORS['hengding'][1]} ({VENDORS['hengding'][0]})."),
        ("h2", "1. Scope 范围"),
        ("p", f"{VENDORS['hengding'][1]} supplies the optical lens set for the {PRODUCT[1]} ({PRODUCT[0]}) smart glasses."),
        ("h2", "2. Price 价格"),
        ("p", f"The total contract value is CNY {fmt(2480000)} (人民币二百四十八万元). A deposit of 30 percent, CNY {fmt(744000)}, is due within 7 days of signing."),
        ("h2", "3. Quality 质量"),
        ("p", "Lens yield of delivered batches must be at least 85 percent. 镜片批次良率不得低于百分之八十五。 Delivery lead time is 21 days after purchase order."),
        ("h2", "4. Parties 当事人"),
        ("p", f"For {COMPANY[1]}: {gu[1]} ({gu[0]}), VP Product. Prepared by {cole[1]}, Legal Counsel. Supply contact: {jiang[1]} ({jiang[0]})."),
    ])
    put_doc("contracts/hengding-lens-supply-contract.pdf", contract, "pdf")
    quote = doc("Meridian Packaging quotation", [("h1", "Quotation Q-2027-0215 Meridian Packaging 子午线包装"),
        ("p", f"Prepared for {raman[1]} and {jiang[1]}, {COMPANY[1]}. Date 2027-02-15."),
        ("p", f"Retail box for {PRODUCT[1]}, first run: CNY {fmt(240000)}. Inserts: CNY {fmt(120000)}. Valid until 2027-03-01.")])
    put_doc("contracts/meridian-packaging-quote.pdf", quote, "pdf")
    rep = [("h1", "Launch Readiness Report 发布就绪报告"),
           ("p", f"Date 2027-02-20. Author: {bai[1]} ({bai[0]}), Program Manager. Product: {PRODUCT[1]}."),
           ("h2", "Summary"),
           ("p", f"The launch date is {LAUNCH_NEW}, moved from {LAUNCH_OLD} after {gu[1]} approved the delay on 2027-01-12 because lens yield was 71 percent against an 85 percent threshold. Yield is now 88 percent."),
           ("p", f"{VENDORS['hengding'][1]} ({VENDORS['hengding'][0]}) is the primary lens supplier under a CNY {fmt(2480000)} contract."),
           ("h2", "Budget table 预算表 (CNY)")]
    rep += [("p", f"{k}   {fmt(v)}   {v * 100 / TOTAL:.1f} percent") for k, v in SUM.items()]
    rep += [("p", f"TOTAL   {fmt(TOTAL)}   100.0 percent"),
            ("p", f"Source: launch-budget-2027.xlsx, maintained by {shao[1]} ({shao[0]}). Marketing was rebalanced on 2027-01-26: CNY 80,000 moved from print to video production."),
            ("h2", "Open risks"), ("p", f"Press embargo lifts 2027-03-16 (owner {raman[1]}). QA go/no-go on 2027-03-05 (owner {ke[1]}).")]
    put_doc("reports/launch-readiness-report-2027-02-20.pdf", doc("Launch Readiness Report", rep), "pdf")
    # scanned memo (image only)
    memo_lines = ["备忘录 MEMO", "", "发件人：顾明远（产品副总裁）", "日期：2027-01-13", f"事由：极光 NP-7 发布日期调整", "",
                  f"发布日期由 {LAUNCH_OLD} 调整为 {LAUNCH_NEW}。", "原因：镜片良率 71%，低于 85% 的门槛。", "决定于 2027-01-12 项目同步会作出。",
                  "", "请白若冰更新项目计划。", "请邵婉清确认总预算 6,850,000 元不变。", "", "", "签名：顾明远   Mingyuan Gu"]
    memo_text = "\n".join(memo_lines)
    ops, y = [{"k": "rect", "x": 60, "y": 70, "w": 1120, "h": 3, "fill": [0.2, 0.2, 0.2]}], 140
    for ln in memo_lines:
        ops.append({"k": "text", "x": 110, "y": y, "size": 34 if ln.startswith("备忘录") else 30, "text": ln, "color": [0.1, 0.1, 0.12]})
        y += 62
    ops.append({"k": "rect", "x": 760, "y": y - 150, "w": 300, "h": 110, "stroke": [0.7, 0.1, 0.1], "lw": 5})
    scan = {"out": f"{tmp}/memo.gray", "format": "gray", "w": 1240, "h": 1754, "seed": 7, "noise": 0.03, "rotate": 0.6, "bg": [0.97, 0.96, 0.93], "ops": ops}
    render([scan], tmp)
    put("memos/launch-delay-memo-signed.pdf", scanned_pdf(Path(f"{tmp}/memo.gray").read_bytes(), 1240, 1754), memo_text)
    # ---------------- docx / pptx
    m1 = doc("Launch sync minutes 2027-01-12 项目同步会纪要", [
        ("h1", "项目同步会纪要 Launch sync minutes"), ("p", f"日期：2027-01-12　产品：{PRODUCT[0]}　记录：{bai[0]}"),
        ("h2", "参会人员"), ("p", f"{gu[0]}、{bai[0]}、{ortiz[0]}（Daniel Ortiz）、{raman[0]}（Priya Raman）、{ke[0]}"),
        ("h2", "决定 Decisions"),
        ("li", f"决定一：由于镜片良率只有 71%，低于 85% 的门槛，{gu[0]} 决定将发布日期由 {LAUNCH_OLD} 推迟到 {LAUNCH_NEW}。"),
        ("li", f"决定二：新闻解禁日期改为 2027-03-16，由 {raman[0]} 负责通知云帆传播。"),
        ("li", f"决定三：向恒鼎光电（Hengding Optoelectronics）询价，由 {VENDORS['hengding'] and jiang[0]} 跟进。"),
        ("h2", "待办事项"), ("trow", ["事项", "负责人", "截止日期"]),
        ("trow", ["发布延期备忘录签发", gu[0], "2027-01-13"]), ("trow", ["更新项目计划", bai[0], "2027-01-14"]),
        ("trow", ["通知云帆传播新闻解禁日", raman[0], "2027-01-12"])])
    put_doc("meetings/minutes-2027-01-12-launch-sync.docx", m1, "docx")
    m2 = doc("Budget review minutes 2027-01-26", [
        ("h1", "Budget review minutes 预算评审纪要"), ("p", f"Date: 2027-01-26. Product: {PRODUCT[1]}. Recorder: {shao[1]}."),
        ("h2", "Attendees"), ("p", f"{gu[1]}, {shao[1]}, {raman[1]}, {jiang[1]}"),
        ("h2", "Decisions"),
        ("li", f"Decision 3: {gu[1]} approved moving CNY 80,000 from print to video production, contracted to {VENDORS['yunfan'][1]}."),
        ("li", f"Total budget stays CNY {fmt(TOTAL)}; marketing becomes CNY {fmt(SUM['Marketing'])} and the contingency reserve stays CNY {fmt(450000)}."),
        ("li", f"The Hengding contract (CNY {fmt(2480000)}) was signed on 2027-01-20 and is booked under Hardware."),
        ("h2", "Actions"), ("trow", ["Action", "Owner", "Due"]),
        ("trow", ["Update the budget workbook", shao[1], "2027-02-02"]), ("trow", ["Sign video production PO", raman[1], "2027-01-27"])])
    put_doc("meetings/minutes-2027-01-26-budget-review.docx", m2, "docx")
    m3 = doc("Readiness check minutes 2027-02-02 就绪检查纪要", [
        ("h1", "就绪检查纪要 Readiness check minutes"), ("p", f"日期：2027-02-02　产品：{PRODUCT[0]}　参会：{bai[0]}、{ke[0]}、{ortiz[0]}、{jiang[0]}"),
        ("h2", "结论"),
        ("li", f"镜片良率由 71% 提升到 88%，高于 85% 的门槛。"),
        ("li", f"质量放行评审（go/no-go）定在 2027-03-05，负责人：{ke[0]}（Ziqian Ke）。"),
        ("li", f"发布日期保持 {LAUNCH_NEW}，新闻解禁 2027-03-16，负责人 {raman[0]}。"),
        ("li", f"速达快运的空运批次于 2027-03-10 到仓，由 {jiang[0]} 跟踪。")])
    put_doc("meetings/minutes-2027-02-02-readiness.docx", m3, "docx")
    spec = doc("Aurora NP-7 product spec 产品规格", [
        ("h1", f"{PRODUCT[1]} ({PRODUCT[0]}) product specification"), ("p", f"Owner: {ortiz[1]}, Hardware Lead. Design: {mao[1]} ({mao[0]}). Version 0.9."),
        ("h2", "Key figures"), ("trow", ["Item", "Value"]), ("trow", ["Weight", "38 g"]), ("trow", ["Battery life", "9 hours"]),
        ("trow", ["Retail price", "CNY 3,999"]), ("trow", ["Lens supplier", f"{VENDORS['hengding'][1]}"]), ("trow", ["Planned launch", LAUNCH_NEW]),
        ("h2", "概述"), ("p", f"极光 NP-7 是一款 38 克的智能眼镜，续航 9 小时，建议零售价 3,999 元。镜片由恒鼎光电供货。")])
    put_doc("specs/np7-product-spec.docx", spec, "docx")
    plan = doc("NP-7 test plan", [
        ("h1", "NP-7 test plan 测试计划"), ("p", f"Owner: {ke[1]} ({ke[0]}), QA Engineer."),
        ("li", "Lens yield: first batch 71 percent, current batch 88 percent, release threshold 85 percent."),
        ("li", f"Go/no-go review: 2027-03-05, chaired by {ke[1]} with {ortiz[1]}."),
        ("li", f"Field test: 40 devices over 14 days, report to {bai[1]}.")])
    put_doc("specs/np7-test-plan.docx", plan, "docx")
    deck = doc("Aurora NP-7 launch plan", [
        ("slide", f"{PRODUCT[1]} launch plan 发布计划", [f"Owner: {gu[1]} ({gu[0]})", f"Program: {bai[1]}"], None),
        ("slide", "Timeline 时间线", [f"Launch {LAUNCH_NEW} (moved from {LAUNCH_OLD}, decision of 2027-01-12 launch sync)", "QA go/no-go 2027-03-05", "Press embargo 2027-03-16"], None),
        ("slide", "Budget 预算", [f"Total CNY {fmt(TOTAL)}", f"Marketing CNY {fmt(SUM['Marketing'])}", "CNY 80,000 moved from print to video on 2027-01-26"], None),
        ("slide", "Supply 供应", [f"{VENDORS['hengding'][1]} 恒鼎光电: lens, CNY {fmt(2480000)}", f"Owner {jiang[1]}", "Yield 71 to 88 percent"], None),
        ("slide", "Marketing 市场", [f"{raman[1]} leads press and video", f"{VENDORS['yunfan'][1]} shoots on 2027-02-10"], None)])
    put_doc("decks/aurora-launch-plan.pptx", deck, "pptx")
    vdeck = doc("Vendor review", [
        ("slide", "Vendor review 供应商评审", [f"Prepared by {jiang[1]}"], None),
        ("slide", "Lens: Hengding 恒鼎光电", ["Selected as primary lens supplier on 2027-01-16", f"Contract CNY {fmt(2480000)}, lead time 21 days"], None),
        ("slide", "Packaging: Meridian 子午线包装", ["Quote Q-2027-0215", f"Retail box CNY {fmt(240000)}"], None),
        ("slide", "Logistics: Suda Express 速达快运", ["Air freight block CNY 520,000", "Arrival 2027-03-10"], None)])
    put_doc("decks/vendor-review.pptx", vdeck, "pptx")
    # ---------------- images
    wb_lines = [f"{PRODUCT[1]}  LAUNCH {LAUNCH_NEW}", f"Press embargo 03-16  ({raman[1]})", f"QA go/no-go 03-05  ({ke[1]})",
                "Lens yield 71% -> 88%", f"Owner: {gu[1]}", f"Video shoot 02-10 - Yunfan Media"]
    ops = [{"k": "rect", "x": 20, "y": 20, "w": 1160, "h": 760, "stroke": [0.55, 0.55, 0.6], "lw": 10}]
    for i, ln in enumerate(wb_lines):
        ops.append({"k": "text", "x": 80, "y": 150 + i * 105, "size": 54, "font": "MarkerFelt-Thin", "text": ln, "color": [0.1, 0.2, 0.55 if i % 2 else 0.1]})
    render([{"out": f"{tmp}/wb.png", "w": 1200, "h": 800, "seed": 11, "noise": 0.05, "rotate": -1.2, "bg": [0.93, 0.94, 0.92], "ops": ops}], tmp)
    put("images/whiteboard-launch-timeline.png", Path(f"{tmp}/wb.png").read_bytes(), "\n".join(wb_lines))
    trk = [("Task", "Owner", "Due", "Status")]
    trk += [("Packaging sign-off", raman[1], "2027-02-25", "In progress"), ("Field test report", ke[1], "2027-03-01", "Open"), ("Air freight booking", jiang[1], "2027-02-03", "Done"), ("Launch video shoot", raman[1], "2027-02-10", "Done")]
    ops = [{"k": "rect", "x": 0, "y": 0, "w": 1280, "h": 56, "fill": [0.16, 0.18, 0.24]},
           {"k": "text", "x": 24, "y": 38, "size": 26, "font": "Helvetica-Bold", "text": f"Task tracker - {PRODUCT[1]} launch", "color": [1, 1, 1]}]
    for i, row in enumerate(trk):
        for j, c in enumerate(row):
            ops.append({"k": "text", "x": 40 + j * 300, "y": 130 + i * 70, "size": 28, "font": "Helvetica-Bold" if i == 0 else "Helvetica", "text": c, "color": [0.1, 0.1, 0.1]})
        ops.append({"k": "rect", "x": 30, "y": 150 + i * 70, "w": 1220, "h": 1, "fill": [0.75, 0.75, 0.78]})
    render([{"out": f"{tmp}/ss.png", "w": 1280, "h": 520, "seed": 5, "bg": [0.98, 0.98, 0.99], "ops": ops}], tmp)
    put("images/screenshot-task-tracker.png", Path(f"{tmp}/ss.png").read_bytes(), "\n".join(" ".join(r) for r in trk))
    # ---------------- audio (macOS `say`, two voices each; clip < 60 s)
    A = {
     "meetings/audio/launch-sync-2027-01-12.wav": ("wav", [("Eddy (英语（美国）)", 175, "This is the Aurora NP-7 launch sync on January twelfth, twenty twenty seven. Daniel Ortiz, where are we on lens yield?"),
        ("Flo (英语（美国）)", 175, "Lens yield from the current supplier is seventy one percent. The threshold is eighty five percent. Mingyuan Gu decided this morning to move the launch from March fourth to March eighteenth."),
        ("Eddy (英语（美国）)", 175, "Understood. This is Priya Raman. Marketing will move the press embargo to March sixteenth, and I will tell Yunfan Media today. Yunfan Media shoots the launch video on February tenth."),
        ("Flo (英语（美国）)", 175, "Ruobing Bai will send a signed memo tomorrow. We will also ask Hengding Optoelectronics for a quote.")],
        "Aurora NP-7 launch sync January 12 2027. Daniel Ortiz. lens yield 71 percent threshold 85 percent. Mingyuan Gu moved launch from March 4 to March 18. Priya Raman press embargo March 16, Yunfan Media shoots launch video February 10. Ruobing Bai signed memo. Hengding Optoelectronics quote. 2027-03-18 2027-03-16 2027-02-10"),
     "meetings/audio/budget-call-2027-01-26.m4a": ("m4a", [("Flo (中文（中国大陆）)", 190, "这里是极光 NP-7 预算评审，日期是二零二七年一月二十六日。我是邵婉清。总预算是六百八十五万元。"),
        ("Eddy (中文（中国大陆）)", 190, "好，我是顾明远。我决定把印刷预算八万元挪到视频制作，交给云帆传播。"),
        ("Flo (中文（中国大陆）)", 190, "收到。市场预算调整后是一百四十二万元，备用金四十五万元保持不变。"),
        ("Eddy (中文（中国大陆）)", 190, "恒鼎光电的合同金额是二百四十八万元，一月二十日已经签署。"),
        ("Flo (中文（中国大陆）)", 190, "我会在二月二日前更新预算表。")],
        "极光 NP-7 预算评审 二零二七年一月二十六日 邵婉清 总预算 六百八十五万元 顾明远 印刷预算八万元 视频制作 云帆传播 市场预算 一百四十二万元 备用金四十五万元 恒鼎光电 二百四十八万元 二月二日 预算表 2027-01-26 2027-02-02"),
     "meetings/audio/supplier-call-2027-01-16.wav": ("wav", [("Daniel", 170, "This is the supplier review on January sixteenth. I am Tianyou Jiang. Hengding Optoelectronics quoted a total of two million four hundred eighty thousand yuan for the lens sets."),
        ("Karen", 170, "Hannah Cole from legal. I will have the contract ready to sign on January twentieth. Mingyuan Gu approved Hengding as our primary lens supplier."),
        ("Daniel", 170, "Hengding promised a delivery lead time of twenty one days after each purchase order, and a yield of at least eighty five percent.")],
        "supplier review January 16 Tianyou Jiang Hengding Optoelectronics two million four hundred eighty thousand yuan Hannah Cole legal contract January twentieth Mingyuan Gu primary lens supplier lead time twenty one days eighty five percent 2,480,000"),
     "meetings/audio/readiness-call-2027-02-02.m4a": ("m4a", [("Eddy (中文（中国大陆）)", 190, "这里是二月二日的就绪检查。我是白若冰。柯子骞，镜片良率现在多少？"),
        ("Flo (中文（中国大陆）)", 190, "我是柯子骞。良率提高到了百分之八十八，高于百分之八十五的门槛。质量放行评审定在三月五日。"),
        ("Eddy (中文（中国大陆）)", 190, "好。发布日期保持三月十八日，蒋天佑负责跟踪速达快运的空运。")],
        "就绪检查 二月二日 白若冰 柯子骞 良率 百分之八十八 百分之八十五 质量放行评审 三月五日 发布日期 三月十八日 蒋天佑 速达快运 空运 2027-03-05 2027-03-18"),
    }
    for path, (kind, segs, txt) in A.items():
        data = say_mix(segs, path, tmp, kind)
        tmpf = Path(tmp) / ("probe." + kind)
        tmpf.write_bytes(data)
        assert duration(tmpf) < 58, (path, duration(tmpf))
        put(path, data, txt)
    # ---------------- notes (md / txt)
    notes = {
     "notes/press-faq-en.md": f"""---\ntitle: Press FAQ\nowner: {raman[1]}\n---\n\n# {PRODUCT[1]} press FAQ\n\n- Launch date: {LAUNCH_NEW}. The press embargo lifts on 2027-03-16.\n- Price: CNY 3,999. Battery life 9 hours, weight 38 g.\n- Spokesperson: {raman[1]}, Head of Marketing. Product owner: {gu[1]}.\n- Lenses are made by {VENDORS['hengding'][1]}.\n""",
     "notes/press-faq-zh.md": f"# {PRODUCT[0]} 新闻问答\n\n- 发布日期：{LAUNCH_NEW}，新闻解禁日期 2027-03-16。\n- 价格 3,999 元，续航 9 小时，重量 38 克。\n- 发言人：{raman[0]}；产品负责人：{gu[0]}。\n",
     "notes/risk-register.md": f"# Risk register\n\n| Risk | Owner | Status |\n|---|---|---|\n| Lens yield below 85 percent | {ke[1]} | Closed, yield 88 percent |\n| Air freight delay | {jiang[1]} | Open, arrival 2027-03-10 |\n| Press embargo leak | {raman[1]} | Open |\n| Budget overrun beyond CNY 450,000 reserve | {shao[1]} | Open |\n",
     "notes/okr-2027-q1.md": f"# Q1 2027 OKR\n\nObjective: launch {PRODUCT[1]} on {LAUNCH_NEW}.\n\n- KR1: lens yield at least 85 percent (owner {ke[1]}).\n- KR2: stay within CNY {fmt(TOTAL)} (owner {shao[1]}).\n- KR3: 3 tier-one press reviews (owner {raman[1]}).\n",
     "notes/okr-2027-q1-zh.md": f"# 2027 年第一季度目标\n\n目标：{LAUNCH_NEW} 发布{PRODUCT[0]}。\n\n- 关键结果一：镜片良率不低于 85%，负责人 {ke[0]}。\n- 关键结果二：预算控制在 {fmt(TOTAL)} 元以内，负责人 {shao[0]}。\n",
     "notes/glossary.md": f"# Glossary 术语表\n\n- Yield 良率: share of lenses passing inspection.\n- Embargo 解禁: date after which press may publish.\n- Go/no-go 放行评审: quality gate before shipping.\n- Aurora NP-7 极光 NP-7: the product.\n",
     "notes/design-notes.md": f"# Design notes\n\nLead: {mao[1]} ({mao[0]}). The frame comes in matte graphite. Retail box by {VENDORS['meridian'][1]}. Hinge review with {ortiz[1]} on 2027-02-12.\n",
     "notes/naming-guidelines.md": f"# Naming\n\nProduct name {PRODUCT[1]} is always written with a hyphen. Chinese name {PRODUCT[0]}. Company {COMPANY[1]} ({COMPANY[0]}).\n",
     "notes/customer-feedback.txt": f"Beta feedback, 12 testers, week of 2027-02-08.\nThe frame felt light (38 g). Battery lasted about 9 hours. Two testers asked for a green frame colour.\nCollected by {bai[1]}.\n",
     "notes/slack-export-launch.txt": f"[2027-01-12 09:14] {gu[1]}: Lens yield is 71 percent. We move launch to {LAUNCH_NEW}.\n[2027-01-12 09:20] {raman[1]}: Embargo moves to 03-16. I will brief Yunfan Media.\n[2027-01-13 10:02] {bai[1]}: Signed memo is out.\n",
     "notes/shipping-schedule.txt": f"Air freight with {VENDORS['suda'][1]}: booked 2027-02-03 by {jiang[1]}, arrival at the warehouse 2027-03-10, CNY 520,000.\n",
     "notes/onboarding-launch-team.md": f"# Launch team onboarding\n\nProgram manager {bai[1]} runs the weekly sync. Finance questions go to {shao[1]}. Contract questions go to {cole[1]}.\n",
     "notes/retro-prep.md": "# Retro prep\n\nWhat went well: fast decision on the delay. What to improve: earlier yield sampling.\n",
     "notes/meeting-cadence.txt": "Weekly launch sync on Tuesdays 10:00. Budget review monthly. Readiness check biweekly.\n",
     "notes/launch-checklist-zh.md": f"# 发布检查清单\n\n- [x] 备忘录签发（{gu[0]}，2027-01-13）\n- [x] 恒鼎光电合同签署（2027-01-20）\n- [ ] 质量放行评审（{ke[0]}，2027-03-05）\n- [ ] 新闻解禁（{raman[0]}，2027-03-16）\n",
    }
    for p, t in notes.items():
        put(p, t)
    # ---------------- unrelated
    recipe = doc("Lemon tart", [("h1", "Grandma's lemon tart"), ("p", "Serves 8. Mix 200 g flour, 100 g butter and one egg for the base. Blind bake for 15 minutes."),
        ("li", "Filling: 4 lemons, 150 g sugar, 3 eggs, 100 ml cream."), ("li", "Bake 25 minutes at 170 degrees. Cool before serving.")])
    put_doc("misc/lemon-tart-recipe.docx", recipe, "docx")
    trip = doc("Lisbon weekend", [("h1", "Lisbon weekend itinerary"), ("p", "Friday: tram 28 and the old town. Saturday: Belem tower and pasteis de nata. Sunday: Sintra day trip by train."),
        ("p", "Hotel near Rossio, check in 15:00. Flight home Monday 07:40.")])
    put_doc("misc/lisbon-itinerary.pdf", trip, "pdf")
    rows, s = [["sample", "x", "y", "z"]], 12345
    for i in range(1, 41):
        s = (s * 1103515245 + 12345) % 2147483648
        a = s % 1000
        s = (s * 1103515245 + 12345) % 2147483648
        rows.append([f"s{i}", a, s % 1000, (a * 7 + s) % 1000])
    put("misc/random-readings.csv", csv_text(rows))
    put("misc/empty-notes.txt", b"", "")
    put("misc/broken-report.pdf", b"%PDF-1.4\n1 0 obj\n<< /Type /Catalog /Pages 2 0 R >>\nendobj\n" + bytes((i * 37) % 256 for i in range(300)), "")
    put("misc/brand-mockup.key", W.zip_bytes([("Index/Document.iwa", bytes((i * 11) % 256 for i in range(2000))), ("Metadata/Properties.plist", "<plist/>")]), "")
    put("misc/icon-set.sketch", W.zip_bytes([("document.json", "{}"), ("meta.json", '{"app":"sketch"}')]), "")


# ================================================================== gold
def gold():
    N = lambda k: P(k)
    f = {"roster": "people/team-roster.xlsx", "directory": "people/contact-directory.csv", "budget": "finance/launch-budget-2027.xlsx", "vendors": "finance/vendor-list.xlsx",
         "actuals": "finance/actuals-2027-q1.csv", "po": "finance/po-log.csv", "contract": "contracts/hengding-lens-supply-contract.pdf", "quote": "contracts/meridian-packaging-quote.pdf",
         "memo": "memos/launch-delay-memo-signed.pdf", "report": "reports/launch-readiness-report-2027-02-20.pdf",
         "m1": "meetings/minutes-2027-01-12-launch-sync.docx", "m2": "meetings/minutes-2027-01-26-budget-review.docx", "m3": "meetings/minutes-2027-02-02-readiness.docx",
         "a1": "meetings/audio/launch-sync-2027-01-12.wav", "a2": "meetings/audio/budget-call-2027-01-26.m4a", "a3": "meetings/audio/supplier-call-2027-01-16.wav", "a4": "meetings/audio/readiness-call-2027-02-02.m4a",
         "spec": "specs/np7-product-spec.docx", "plan": "specs/np7-test-plan.docx", "deck": "decks/aurora-launch-plan.pptx", "vdeck": "decks/vendor-review.pptx",
         "wb": "images/whiteboard-launch-timeline.png", "ss": "images/screenshot-task-tracker.png", "faq": "notes/press-faq-en.md", "faqzh": "notes/press-faq-zh.md",
         "risk": "notes/risk-register.md", "slack": "notes/slack-export-launch.txt", "ship": "notes/shipping-schedule.txt", "check": "notes/launch-checklist-zh.md", "okr": "notes/okr-2027-q1.md"}
    R = []

    def rel(id_, kind, label, names, files, kinds=None):
        R.append({"id": id_, "kind": kind, "label": label, "names": names, "files": [f[x] for x in files]})

    def person(id_, k, files):
        rel(id_, "same_person", f"{N(k)[0]} / {N(k)[1]} across files", [N(k)[0], N(k)[1]], files)
    person("r01", "gu", ["roster", "m1", "memo", "deck", "a2", "a3", "directory", "faq", "slack", "po"])
    person("r02", "bai", ["roster", "m1", "memo", "deck", "a1", "a4", "report", "plan"])
    person("r03", "raman", ["roster", "m1", "a1", "deck", "wb", "faq", "slack", "po", "ss"])
    person("r04", "ortiz", ["roster", "a1", "spec", "plan", "m3"])
    person("r05", "shao", ["roster", "budget", "a2", "m2", "report"])
    person("r06", "jiang", ["roster", "vendors", "a3", "vdeck", "ship", "m3"])
    person("r07", "cole", ["roster", "contract", "a3"])
    person("r08", "ke", ["roster", "plan", "m3", "wb", "a4", "ss"])
    person("r09", "mao", ["roster", "spec"])
    h = VENDORS["hengding"]
    rel("r10", "vendor", "Hengding Optoelectronics / 恒鼎光电 across files", [h[0], h[1], "Hengding"], ["vendors", "contract", "m1", "a3", "vdeck", "budget", "report", "actuals", "po", "spec", "deck"])
    y = VENDORS["yunfan"]
    rel("r11", "vendor", "Yunfan Media / 云帆传播", [y[0], y[1]], ["vendors", "a1", "m2", "actuals", "po", "m1", "deck"])
    rel("r12", "vendor", "Meridian Packaging / 子午线包装", [VENDORS["meridian"][0], VENDORS["meridian"][1]], ["vendors", "quote", "vdeck", "po"])
    rel("r13", "vendor", "Suda Express / 速达快运", [VENDORS["suda"][0], VENDORS["suda"][1]], ["vendors", "actuals", "po", "vdeck", "ship", "a4", "m3"])
    rel("r14", "product", "Aurora NP-7 / 极光 NP-7", [PRODUCT[1], PRODUCT[0]], ["spec", "deck", "report", "memo", "m1", "m3", "faq", "faqzh", "wb", "a1", "a2"])
    rel("r15", "decision", "Decision to move the launch from 2027-03-04 to 2027-03-18", [LAUNCH_NEW, "03-18", "March eighteenth", "三月十八日"], ["m1", "memo", "deck", "a1", "report", "wb", "slack", "a4"])
    rel("r16", "decision", "Decision to select Hengding as primary lens supplier", [h[1], h[0]], ["a3", "contract", "vdeck", "report", "m1"])
    rel("r17", "decision", "Decision to move CNY 80,000 from print to video", ["80,000", "80000", "八万元", "eighty thousand"], ["m2", "a2", "budget", "deck", "report"])
    rel("r18", "figure", "Total launch budget 6,850,000", ["6,850,000", "6850000", "六百八十五万", "six point eight five million"], ["budget", "report", "deck", "a2", "memo"])
    rel("r19", "figure", "Marketing budget 1,420,000", ["1,420,000", "1420000", "一百四十二万"], ["budget", "report", "deck", "a2", "m2"])
    rel("r20", "figure", "Hengding contract value 2,480,000", ["2,480,000", "2480000", "二百四十八万", "two million four hundred eighty thousand"], ["contract", "budget", "vendors", "a3", "a2", "deck", "report", "m2"])
    rel("r21", "date", "Press embargo 2027-03-16", ["2027-03-16", "03-16", "March sixteenth", "三月十六"], ["m1", "a1", "wb", "faq", "faqzh", "deck", "report", "check"])
    rel("r22", "date", "QA go/no-go 2027-03-05", ["2027-03-05", "03-05", "三月五日"], ["m3", "plan", "wb", "deck", "a4", "report", "check"])
    rel("r23", "figure", "Lens yield 71 percent rising to 88 percent against an 85 percent threshold", ["85", "88%", "71", "百分之八十五"], ["m1", "memo", "plan", "report", "a1", "a4", "m3", "contract"])
    rel("r24", "date", "Launch video shoot 2027-02-10", ["2027-02-10", "February tenth", "02-10"], ["a1", "ss", "deck"])
    rel("r25", "date", "Air freight arrival 2027-03-10", ["2027-03-10", "03-10"], ["ship", "vdeck", "m3"])
    return R


def gold_unlinked():
    u = lambda p, k, why: {"file": "misc/" + p, "kind": k, "expected_reason": why}
    return [u("lemon-tart-recipe.docx", "unrelated_content", "parsed but shares no entity with the project"),
            u("lisbon-itinerary.pdf", "unrelated_content", "parsed but shares no entity with the project"),
            u("random-readings.csv", "unrelated_content", "numeric table with no names, dates or entities shared with the project"),
            u("empty-notes.txt", "empty_file", "file has no content"),
            u("broken-report.pdf", "corrupt_file", "PDF structure is damaged and cannot be read"),
            u("brand-mockup.key", "unsupported_format", "Keynote package is not a supported format"),
            u("icon-set.sketch", "unsupported_format", "Sketch file is not a supported format")]


def gold_questions():
    f = lambda *x: list(x)
    q = []

    def add(kind, question, answer, files, lang="en", answerable=True):
        q.append({"id": f"q{len(q) + 1:02d}", "kind": kind, "lang": lang, "question": question, "answer": answer, "answerable": answerable, "files": files})
    add("single", "What is the current planned launch date of the Aurora NP-7?", "2027-03-18", f("meetings/minutes-2027-01-12-launch-sync.docx"))
    add("single", "What lens yield was reported at the 2027-01-12 launch sync, and what was the threshold?", "71 percent against an 85 percent threshold", f("meetings/minutes-2027-01-12-launch-sync.docx"))
    add("single", "Who is the Hardware Lead?", "Daniel Ortiz", f("people/team-roster.xlsx"))
    add("single", "What is the total value of the Hengding lens supply contract?", "CNY 2,480,000", f("contracts/hengding-lens-supply-contract.pdf"))
    add("single", "What is the retail price of the Aurora NP-7?", "CNY 3,999", f("specs/np7-product-spec.docx"))
    add("single", "What battery life does the product spec state?", "9 hours", f("specs/np7-product-spec.docx"))
    add("single", "How much does the Aurora NP-7 weigh?", "38 g", f("specs/np7-product-spec.docx"))
    add("single", "Which team is Ziqian Ke in?", "Quality", f("people/team-roster.xlsx"))
    add("cross", "Who decided to delay the launch and where was it formalised?", "Mingyuan Gu decided it at the 2027-01-12 launch sync; the signed memo of 2027-01-13 formalised it", f("meetings/minutes-2027-01-12-launch-sync.docx", "memos/launch-delay-memo-signed.pdf"))
    add("cross", "Which vendor supplies the lenses and who inside the company owns that relationship?", "Hengding Optoelectronics; Tianyou Jiang", f("finance/vendor-list.xlsx", "contracts/hengding-lens-supply-contract.pdf", "people/team-roster.xlsx"))
    add("cross", "Does the total budget in the readiness report match the budget workbook?", "Yes, both give CNY 6,850,000", f("reports/launch-readiness-report-2027-02-20.pdf", "finance/launch-budget-2027.xlsx"))
    add("cross", "Who owns marketing and what is the marketing budget?", "Priya Raman; CNY 1,420,000", f("people/team-roster.xlsx", "finance/launch-budget-2027.xlsx"))
    add("cross", "Which meetings discussed moving CNY 80,000 from print to video?", "The 2027-01-26 budget review (minutes and the Chinese budget call recording)", f("meetings/minutes-2027-01-26-budget-review.docx", "meetings/audio/budget-call-2027-01-26.m4a"))
    add("cross", "Which slide of the launch deck covers budget and what total does it show?", "The Budget slide, total CNY 6,850,000", f("decks/aurora-launch-plan.pptx", "finance/launch-budget-2027.xlsx"))
    add("cross", "What is the press embargo date and who owns the press plan?", "2027-03-16; Priya Raman", f("meetings/minutes-2027-01-12-launch-sync.docx", "people/team-roster.xlsx"))
    add("cross", "When is the QA go/no-go review and who owns it?", "2027-03-05; Ziqian Ke", f("meetings/minutes-2027-02-02-readiness.docx", "specs/np7-test-plan.docx"))
    add("cross", "Which vendor was hired for video and ads and what is its contract value?", "Yunfan Media; CNY 880,000", f("finance/vendor-list.xlsx", "finance/launch-budget-2027.xlsx"))
    add("cross", "Which legal counsel prepared the Hengding contract?", "Hannah Cole", f("contracts/hengding-lens-supply-contract.pdf", "people/team-roster.xlsx"))
    add("numeric", "What is the Logistics budget in the budget workbook?", "CNY 980,000", f("finance/launch-budget-2027.xlsx"))
    add("numeric", "What is the print budget after the reallocation?", "CNY 140,000", f("finance/launch-budget-2027.xlsx"))
    add("numeric", "What is the contingency reserve?", "CNY 450,000", f("finance/launch-budget-2027.xlsx"))
    add("numeric", "What share of the total budget is Hardware, in percent?", "45.3 percent (3,100,000 of 6,850,000)", f("finance/launch-budget-2027.xlsx"))
    add("numeric", "What are the total marketing actuals for January and February 2027?", "CNY 540,000", f("finance/actuals-2027-q1.csv"))
    add("numeric", "What do video production and digital ads cost together in the marketing budget?", "CNY 880,000", f("finance/launch-budget-2027.xlsx"))
    add("audio", "When does Yunfan Media shoot the launch video?", "2027-02-10", f("meetings/audio/launch-sync-2027-01-12.wav"))
    add("audio", "By what date will Wanqing Shao update the budget sheet, according to the Chinese budget call?", "2027-02-02", f("meetings/audio/budget-call-2027-01-26.m4a"))
    add("audio", "What delivery lead time did Hengding promise in the supplier call?", "21 days after each purchase order", f("meetings/audio/supplier-call-2027-01-16.wav"))
    add("crosslang", "Mingyuan Gu 在中文材料里叫什么，职位是什么？", "顾明远，产品副总裁", f("people/team-roster.xlsx", "memos/launch-delay-memo-signed.pdf"), lang="zh")
    add("crosslang", "恒鼎光电的英文名是什么？", "Hengding Optoelectronics", f("finance/vendor-list.xlsx", "contracts/hengding-lens-supply-contract.pdf"), lang="zh")
    add("crosslang", "What yield threshold does the scanned Chinese memo give?", "85 percent", f("memos/launch-delay-memo-signed.pdf"))
    add("unanswerable", "What is the manufacturing cost per unit of the Aurora NP-7?", None, [], answerable=False)
    add("unanswerable", "Who is the CEO of Northport Optics?", None, [], answerable=False)
    add("unanswerable", "How many units were sold in the first week after launch?", None, [], answerable=False)
    add("unanswerable", "What is Hannah Cole's phone number?", None, [], answerable=False)
    return q


def check_gold(rel_gold, unl, qs):
    """The gold must be consistent with the generated sources: every named file exists and mentions one of the names."""
    bad = []
    for r in rel_gold:
        for fpath in r["files"]:
            assert fpath in TEXT, (r["id"], fpath)
            blob = TEXT[fpath].lower()
            if not any(n.lower() in blob for n in r["names"]):
                bad.append((r["id"], fpath))
    assert not bad, bad
    for u in unl:
        assert u["file"] in FILES
    for x in qs:
        for fpath in x["files"]:
            assert fpath in FILES, (x["id"], fpath)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE.parent.parent.parent / "testing/super-ontology/acceptance"))
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    with tempfile.TemporaryDirectory() as tmp:
        build(tmp)
    rg, ug, qg = gold(), gold_unlinked(), gold_questions()
    check_gold(rg, ug, qg)
    dump = lambda rows: "".join(json.dumps(r, ensure_ascii=False, sort_keys=True) + "\n" for r in rows).encode("utf-8")
    out = {f"project/{k}": v for k, v in FILES.items()}
    out.update({"gold/relations.jsonl": dump(rg), "gold/unlinked.jsonl": dump(ug), "gold/questions.jsonl": dump(qg)})
    out["MANIFEST.sha256"] = "".join(f"{hashlib.sha256(v).hexdigest()}  {k}\n" for k, v in sorted(out.items())).encode()
    base = Path(a.out)
    if a.check:
        # audio and scans come from the OS synthesiser and renderer: bytes can differ between OS builds, so --check compares the text side
        bad = [k for k, v in out.items() if not k.startswith("project/meetings/audio/") and not k.endswith(".png") and not k.endswith("signed.pdf") and k != "MANIFEST.sha256"
               and (not (base / k).exists() or (base / k).read_bytes() != v)]
        print("check:", "ok" if not bad else f"differs: {bad}")
        sys.exit(1 if bad else 0)
    if (base / "project").exists():
        shutil.rmtree(base / "project")
    for k, v in out.items():
        p = base / k
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(v)
    total = sum(len(v) for k, v in out.items() if k.startswith("project/"))
    print(f"wrote {len([k for k in out if k.startswith('project/')])} files, {total / 1e6:.2f} MB to {base}")


if __name__ == "__main__":
    main()
