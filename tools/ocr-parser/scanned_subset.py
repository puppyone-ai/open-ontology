#!/usr/bin/env python3
"""Synthetic scanned subset of the golden set and the OCR recall measurements (B-S9-01, 30 L6 acceptance 1).

    python3 -B scanned_subset.py render  --golden testing/super-ontology/golden --out /tmp/scanned
    python3 -B scanned_subset.py measure --golden testing/super-ontology/golden --out /tmp/scanned [--report md]

`render` turns every pdf page into a bitmap (pdfium) and every md, txt, docx and pptx text into page images
drawn with a system CJK font, then degrades them like a scan (grey paper, noise, a small skew, blur). Seeds come
from the file path, so a run is repeatable on the same fonts. `measure` runs PP-StructureV3 over the images in
process (same adapter as the sidecar) and reports

* core-type recall (D15 types): an entity counts when one of its anchor files is in the subset, and is found
  when its name or an alias appears in the OCR text of one of those files. This is the OCR-only share of
  extraction recall: the LLM step is not in this loop.
* character recall on synthetic chart and screenshot images (embedded-image text, B-S9-04): ground-truth characters
  found in the OCR output (multiset overlap) divided by all ground-truth characters.

Needs numpy, Pillow, pypdfium2 and, for `measure`, the ocr-parser dependencies and models (OO_OCR_MODELS).
"""
import argparse
import collections
import hashlib
import json
import random
import re
import sys
import unicodedata
import zipfile
from pathlib import Path

FONTS = ["/System/Library/Fonts/Songti.ttc", "/System/Library/Fonts/STHeiti Medium.ttc",
         "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc", "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"]
W, H = 1654, 2339  # A4 at 200 dpi
SUBSET_SUFFIX = (".pdf", ".md", ".txt", ".docx", ".pptx")


def font(size):
    from PIL import ImageFont

    for f in FONTS:
        if Path(f).exists():
            return ImageFont.truetype(f, size)
    sys.exit("no CJK font found")


def docx_text(path):
    xml = zipfile.ZipFile(path).read("word/document.xml").decode("utf-8")
    paras = re.findall(r"<w:p[ >].*?</w:p>", xml, re.S)
    return ["".join(re.findall(r"<w:t[^>]*>(.*?)</w:t>", p, re.S)) for p in paras if "<w:t" in p]


def pptx_slides(path):
    z = zipfile.ZipFile(path)
    names = sorted((n for n in z.namelist() if re.fullmatch(r"ppt/slides/slide\d+\.xml", n)),
                   key=lambda n: int(re.findall(r"\d+", n)[0]))
    return [re.findall(r"<a:t>(.*?)</a:t>", z.read(n).decode("utf-8"), re.S) for n in names]


def text_pages(lines, title=None):
    """Lay out `lines` on A4 pages; yields PIL images."""
    from PIL import Image, ImageDraw

    f_body, f_title = font(36), font(52)
    pages, img, d, y = [], None, None, 0

    def new_page():
        nonlocal img, d, y
        img = Image.new("L", (W, H), 255)
        d, y = ImageDraw.Draw(img), 200
        pages.append(img)

    new_page()
    items = ([("t", title)] if title else []) + [("b", l) for l in lines]
    for kind, line in items:
        f = f_title if kind == "t" else f_body
        step = 90 if kind == "t" else 62
        wrapped, cur = [], ""
        for ch in line:
            if d.textlength(cur + ch, font=f) > W - 400:
                wrapped.append(cur)
                cur = ch
            else:
                cur += ch
        wrapped.append(cur)
        for seg in wrapped:
            if y + step > H - 200:
                new_page()
            d.text((200, y), seg, font=f, fill=0)
            y += step
        y += 16
    return pages


def scan(img, key):
    import numpy as np
    from PIL import ImageFilter

    rnd = random.Random(int(hashlib.sha256(key.encode()).hexdigest()[:8], 16))
    angle = rnd.uniform(-0.4, 0.4)
    img = img.rotate(angle, resample=3, expand=False, fillcolor=255).filter(ImageFilter.GaussianBlur(0.6))
    a = np.asarray(img, dtype=np.float32) * 0.96
    a += np.random.default_rng(rnd.randrange(1 << 30)).normal(0, 3.0, a.shape)
    from PIL import Image

    return Image.fromarray(np.clip(a, 0, 255).astype("uint8")).convert("RGB")


def render(golden, out):
    import pypdfium2 as pdfium

    out.mkdir(parents=True, exist_ok=True)
    n = 0
    for src in sorted((golden / "docs").rglob("*")):
        if src.suffix not in SUBSET_SUFFIX:
            continue
        rel = src.relative_to(golden).as_posix()
        stem = rel.replace("/", "__")
        pages = []
        if src.suffix == ".pdf":
            doc = pdfium.PdfDocument(str(src))
            pages = [doc[i].render(scale=200 / 72).to_pil() for i in range(len(doc))]
        elif src.suffix in (".md", ".txt"):
            pages = text_pages(src.read_text(encoding="utf-8").splitlines())
        elif src.suffix == ".docx":
            pages = text_pages(docx_text(src))
        else:
            for slide in pptx_slides(src):
                pages += text_pages(slide[1:], title=slide[0] if slide else None)[:1]
        for i, p in enumerate(pages, 1):
            scan(p, f"{rel}#{i}").save(out / f"{stem}__p{i}.png")
            n += 1
    print(f"rendered {n} page images into {out}")


def norm(s):
    return re.sub(r"\s+", "", unicodedata.normalize("NFKC", s)).lower()


def ocr_text(adapter, img_path):
    from PIL import Image

    blocks = adapter._run(Image.open(img_path).convert("RGB"))
    return "".join(b["text"] for b in blocks), blocks


def char_recall(truth, got):
    """Share of ground-truth characters present in the OCR output (multiset overlap, so the reading order of a
    chart's labels does not matter; punctuation is folded by NFKC and spaces are dropped)."""
    t, g = collections.Counter(norm(truth)), collections.Counter(norm(got))
    total = sum(t.values())
    return sum((t & g).values()) / total if total else 1.0


def chart_images(out):
    """Synthetic chart and screenshot images with known text (embedded-image text, B-S9-04)."""
    from PIL import Image, ImageDraw

    d = out / "embedded"
    if (d / "truth.json").exists() and (d / "chart.png").exists():  # rendered on a host with a CJK font
        return json.loads((d / "truth.json").read_text())
    d.mkdir(parents=True, exist_ok=True)
    f, fs = font(30), font(22)
    truths = {}
    img = Image.new("RGB", (900, 560), "white")
    dr = ImageDraw.Draw(img)
    cats = [("北辰云", 82), ("星图看板", 64), ("知了客服", 47), ("官网改版", 31)]
    dr.text((40, 20), "各项目季度完成率（%）", font=f, fill="black")
    for i, (c, v) in enumerate(cats):
        x = 120 + i * 190
        dr.rectangle([x, 480 - v * 4, x + 110, 480], fill=(70, 110, 180))
        dr.text((x + 20, 470 - v * 4 - 40), str(v), font=fs, fill="black")
        dr.text((x - 10, 495), c, font=fs, fill="black")
    img.save(d / "chart.png")
    truths["chart.png"] = "各项目季度完成率（%）" + "".join(f"{v}{c}" for c, v in cats)
    img = Image.new("RGB", (1000, 520), (245, 246, 248))
    dr = ImageDraw.Draw(img)
    rows = ["任务看板 · 灯塔迁移", "待办 3    进行中 5    已完成 12", "负责人：陈子墨    截止日期：2026-09-30",
            "状态：灰度发布，每次放量不超过百分之十", "最近更新：吴启明 提交了回滚预案"]
    for i, r in enumerate(rows):
        dr.text((40, 30 + i * 90), r, font=f, fill=(30, 30, 40))
    img.save(d / "screenshot.png")
    truths["screenshot.png"] = "".join(rows)
    (d / "truth.json").write_text(json.dumps(truths, ensure_ascii=False, indent=1))
    return truths


def measure(golden, out, adapter, report):
    ents = [json.loads(l) for l in (golden / "labels" / "entities.jsonl").read_text().splitlines() if l.strip()]
    pages = sorted(p for p in out.glob("*.png") if not p.name.startswith("."))
    by_file = {}
    for p in pages:
        stem = p.name.rsplit("__p", 1)[0]
        by_file.setdefault(stem, []).append(p)
    texts = {}
    cache = out / "ocr-text.json"
    done = json.loads(cache.read_text()) if cache.exists() else {}
    for stem, ps in by_file.items():
        if stem not in done:
            done[stem] = norm("".join(ocr_text(adapter, p)[0] for p in ps))
            cache.write_text(json.dumps(done, ensure_ascii=False))
        texts[stem] = done[stem]
        print(f"ocr {stem}: {len(texts[stem])} chars", file=sys.stderr)
    scored, hits, misses = 0, 0, []
    by_type = {}
    for e in ents:
        files = {a["file"].replace("/", "__") for a in e["anchors"]} & set(texts)
        if not files:
            continue
        scored += 1
        names = [norm(e["name"])] + [norm(a) for a in e.get("aliases", [])]
        ok = any(n and n in texts[f] for n in names for f in files)
        t = by_type.setdefault(e["type"], [0, 0])
        t[1] += 1
        if ok:
            hits += 1
            t[0] += 1
        else:
            misses.append(e["name"])
    truths = chart_images(out)
    chart = {}
    for name, truth in truths.items():
        got, _ = ocr_text(adapter, out / "embedded" / name)
        chart[name] = round(char_recall(truth, got), 4)
    res = {"files": len(by_file), "pages": len(pages), "entities_scored": scored, "entities_found": hits,
           "core_type_recall": round(hits / scored, 4) if scored else 0.0,
           "by_type": {k: f"{v[0]}/{v[1]}" for k, v in sorted(by_type.items())}, "missed": misses,
           "embedded_char_recall": chart, "embedded_char_recall_mean": round(sum(chart.values()) / len(chart), 4)}
    print(json.dumps(res, ensure_ascii=False, indent=1))
    if report:
        Path(report).write_text(json.dumps(res, ensure_ascii=False, indent=1) + "\n")
    return res


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("cmd", choices=["render", "measure"])
    ap.add_argument("--golden", default="testing/super-ontology/golden")
    ap.add_argument("--out", required=True)
    ap.add_argument("--report")
    a = ap.parse_args()
    golden, out = Path(a.golden), Path(a.out)
    if a.cmd == "render":
        render(golden, out)
        return
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "doc-parser"))
    from ocr_parser import adapter

    measure(golden, out, adapter, a.report)


if __name__ == "__main__":
    main()
