#!/usr/bin/env python3
"""A synthetic picture the OCR reads badly (lane I, Desktop T1 "OCR low-confidence mark").

The engine marks an OCR block `low_text_confidence` when the mean recognition score is under 0.8
(`crates/oo-docs/src/normalize/paddle.rs`). Clear text never gets there, so this makes the other kind of page on
purpose: small grey text on a grey background, blurred, with fixed-seed noise. The recognition itself is the real
ocr-parser (PP-StructureV3); only the input is degraded. Same arguments, same bytes. With the defaults the oo-docs-ocr-parser image answers one block of
mean score 0.68 (measured 2026-11-18; a mark needs < 0.8, a score near 0 means no block at all).

  make_lowconf_image.py OUT.png [--blur 1.0] [--noise 40] [--size 26] [--seed 7]

Needs Pillow and a CJK font (macOS: STHeiti or Songti).
"""
import argparse
import random

LINES = ["2026年度计划会纪要", "负责人：林川", "项目：星火计划", "状态：进行中，九月上线"]


def render(out, blur=1.0, noise=40, size=26, seed=7):
    from PIL import Image, ImageDraw, ImageFilter, ImageFont

    font = None
    for f in ("/System/Library/Fonts/STHeiti Medium.ttc", "/System/Library/Fonts/Songti.ttc",
              "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"):
        try:
            font = ImageFont.truetype(f, size)
            break
        except OSError:
            pass
    if font is None:
        raise SystemExit("no CJK font found")
    img = Image.new("RGB", (900, 420), (176, 176, 176))
    d = ImageDraw.Draw(img)
    for i, t in enumerate(LINES):
        d.text((40, 60 + i * 70), t, font=font, fill=(128, 128, 128))
    img = img.filter(ImageFilter.GaussianBlur(blur))
    rng = random.Random(seed)
    px = img.load()
    for y in range(img.height):
        for x in range(img.width):
            n = rng.randint(-noise, noise)
            r, g, b = px[x, y]
            px[x, y] = (max(0, min(255, r + n)), max(0, min(255, g + n)), max(0, min(255, b + n)))
    img.save(out)


if __name__ == "__main__":
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("out")
    ap.add_argument("--blur", type=float, default=1.0)
    ap.add_argument("--noise", type=int, default=40)
    ap.add_argument("--size", type=int, default=26)
    ap.add_argument("--seed", type=int, default=7)
    a = ap.parse_args()
    render(a.out, a.blur, a.noise, a.size, a.seed)
