"""Synthetic screen recording for tests and the golden set: slide frames at 1 fps (no real people, no real data)."""
import random
from pathlib import Path

FONTS = ["/System/Library/Fonts/Songti.ttc", "/System/Library/Fonts/STHeiti Medium.ttc",
         "/usr/share/fonts/opentype/noto/NotoSerifCJK-Regular.ttc", "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"]

SLIDES = [
    ["季度复盘会议", "星图看板 灯塔迁移 知了客服"],
    ["本季度目标", "月活跃团队数提升百分之二十", "客户满意度不低于九十分"],
    ["星图看板进展", "负责人 陈子墨", "完成核心图表库替换"],
    ["灯塔迁移计划", "负责人 吴启明", "灰度发布 每次放量不超过百分之十"],
    ["知了客服方案", "负责人 周若溪", "采购外部模型 控制成本"],
    ["风险与对策", "迁移窗口冲突", "保留旧机房两周"],
    ["待办事项", "整理年度目标 确认预算", "下周四发送周报"],
    ["谢谢大家", "会议纪要将于今日发出"],
]


def font(size):
    from PIL import ImageFont

    for f in FONTS:
        if Path(f).exists():
            return ImageFont.truetype(f, size), True
    return ImageFont.load_default(), False


def has_cjk_font():
    return font(20)[1]


def slide_image(lines, size=(1280, 720), tick=0, seed=0):
    """One slide; `tick` changes only a small clock in a corner, like the live clock of a screen share."""
    from PIL import Image, ImageDraw

    img = Image.new("RGB", size, (250, 250, 252))
    d = ImageDraw.Draw(img)
    f_title, _ = font(64)
    f_body, _ = font(44)
    f_small, _ = font(20)
    d.rectangle([0, 0, size[0], 90], fill=(40, 70, 140))
    d.text((60, 140), lines[0], font=f_title, fill=(20, 20, 40))
    for i, l in enumerate(lines[1:]):
        d.text((90, 280 + i * 90), l, font=f_body, fill=(40, 40, 60))
    d.text((size[0] - 130, size[1] - 40), f"00:{tick % 60:02d}", font=f_small, fill=(120, 120, 130))
    rnd = random.Random(seed)
    for _ in range(40):  # screen noise: a few specks, far below the hash threshold
        x, y = rnd.randrange(size[0]), rnd.randrange(size[1])
        d.point((x, y), fill=(235, 235, 240))
    return img


def slide_frames(durations_s=None, slides=None, size=(1280, 720)):
    """`[(t_ms, image, slide_index)]` at 1 fps; each slide stays for its duration."""
    slides = slides or SLIDES
    durations_s = durations_s or [6, 8, 5, 7, 6, 5, 6, 4][: len(slides)]
    out, t = [], 0
    for idx, (lines, dur) in enumerate(zip(slides, durations_s)):
        for k in range(dur):
            out.append((t * 1000, slide_image(lines, size, tick=t, seed=t), idx))
            t += 1
    return out


def truth_text(slide_index, slides=None):
    return "".join((slides or SLIDES)[slide_index])
