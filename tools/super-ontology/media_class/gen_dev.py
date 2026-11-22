#!/usr/bin/env python3
"""A separate development set of synthetic meetings for tuning the speaker clustering (lane AL): new voices and pitch bases, new lines, and
single-voice negatives (a promo read, a lecture) that must stay one speaker. Never used for the held-out numbers (`gen_meetings.py`).

    python3 gen_dev.py OUT_DIR
"""
import sys
from pathlib import Path

import gen_meetings as g

Z = lambda n: f"{n} (中文（中国大陆）)"
E = lambda n: f"{n} (英语（美国）)"
SETS = {
    "d-zh2a": [(Z("Grandma"), 165, 45), (Z("Reed"), 190, 25)],
    "d-zh2b": [(Z("Shelley"), 180, 40), (Z("Rocko"), 155, 55)],
    "d-zh2c": [(Z("Eddy"), 175, 35), (Z("Sandy"), 195, 60)],
    "d-zh3a": [(Z("Grandma"), 170, 40), (Z("Rocko"), 190, 30), (Z("Shelley"), 160, 55)],
    "d-zh3b": [(Z("Reed"), 175, 30), (Z("Flo"), 195, 55), (Z("Grandpa"), 155, 40)],
    "d-en2a": [(E("Grandma"), 165, 45), (E("Reed"), 190, 30)],
    "d-en2b": [(E("Eddy"), 175, 40), (E("Flo"), 185, 60)],
    "d-en2c": [(E("Rocko"), 170, 35), (E("Grandpa"), 160, 50)],
    "d-en3a": [(E("Grandma"), 170, 40), (E("Rocko"), 190, 35), (E("Flo"), 160, 60)],
    "d-en3b": [(E("Eddy"), 175, 30), (E("Reed"), 195, 50), (E("Grandpa"), 155, 40)],
}
LINES_ZH = [
    "各位早上好，今天的议程有三项：预算、排期和风险。", "预算这边，第二季度已经用掉百分之六十，剩下的足够支撑到月底。",
    "排期有一点问题，设计评审推迟了两天，会影响后面的联调。", "联调我们可以压缩，把接口文档提前一天交给测试同学。",
    "风险方面，我最担心的是供应商那边的交付，上次就延期了。", "那我今天下午联系他们，确认新的交付日期，再回复大家。",
    "好，散会之后我把纪要和待办发到群里。", "谢谢大家，下周同一时间再见。",
]
LINES_EN = [
    "Hello team, today we have three topics: budget, schedule and risk.", "On budget, we have used sixty percent of the second quarter and the rest will last until month end.",
    "The schedule has one problem, the design review slipped two days and that affects integration.", "We can compress integration if the interface document reaches testing a day earlier.",
    "On risk, my main worry is the supplier delivery, which was late last time.", "Then I will call them this afternoon, confirm the new date and report back.",
    "Fine, after the meeting I will post the minutes and the action items.", "Thanks everyone, see you next week at the same time.",
]
SETS_B = {
    "e-zh2a": [(Z("Tingting"), 160, 30), (Z("Shelley"), 200, 55)],
    "e-zh2b": [(Z("Rocko"), 185, 45), (Z("Grandma"), 150, 25)],
    "e-zh2c": [(Z("Reed"), 170, 50), (Z("Eddy"), 190, 30)],
    "e-zh3a": [(Z("Sandy"), 165, 35), (Z("Grandpa"), 185, 25), (Z("Tingting"), 150, 55)],
    "e-zh3b": [(Z("Shelley"), 175, 45), (Z("Eddy"), 195, 35), (Z("Reed"), 160, 60)],
    "e-en2a": [(E("Reed"), 160, 35), (E("Flo"), 195, 55)],
    "e-en2b": [(E("Grandpa"), 175, 30), (E("Eddy"), 165, 50)],
    "e-en2c": [(E("Rocko"), 190, 45), (E("Grandma"), 155, 30)],
    "e-en3a": [(E("Flo"), 170, 50), (E("Grandpa"), 190, 30), (E("Reed"), 155, 40)],
    "e-en3b": [(E("Rocko"), 165, 35), (E("Grandma"), 185, 55), (E("Eddy"), 150, 45)],
}
LINES2_ZH = [
    "下面讲一下这周的客户反馈，一共收到十二条，其中三条是紧急的。", "紧急的三条都和登录有关，我们周二已经定位到原因，是缓存过期时间设错了。",
    "修复需要多久？能不能在周四之前发布？", "代码已经改完，等测试验证，预计周三下午可以发版。",
    "另外有两位客户想要导出功能，我想把它放进下个版本。", "导出的工作量不小，我建议先做表格格式，其他格式以后再说。",
    "同意，那就这么定，我来写需求说明。", "好的，会后我把客户名单整理给大家。",
]
LINES2_EN = [
    "Let's go through the customer feedback this week, twelve items in total and three of them urgent.", "All three urgent ones are about login, and on Tuesday we traced them to a cache expiry set wrongly.",
    "How long will the fix take, and can it ship before Thursday?", "The code is done and waits for testing, so the release should be Wednesday afternoon.",
    "Two customers also asked for an export feature and I would like it in the next version.", "Export is a lot of work, so I suggest starting with the spreadsheet format and the rest later.",
    "Agreed, let's settle it like that and I will write the requirements note.", "Good, after the meeting I will share the customer list with everyone.",
]
SINGLE = {"d-solo-zh": [(Z("Tingting"), 175, 45)], "d-solo-en": [(E("Fred"), 170, 40)]}


def main(out_dir):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    for name, voices in SETS.items():
        g.make(name, voices, LINES_ZH if "zh" in name else LINES_EN, out)
    for name, voices in SETS_B.items():
        g.make(name, voices, LINES2_ZH if "zh" in name else LINES2_EN, out)
    for name, voices in SINGLE.items():
        # One voice reading the same lines: the reference has a single speaker.
        g.make(name, voices, LINES_ZH if "zh" in name else LINES_EN, out)
    print(sorted(p.name for p in out.glob("*.wav")))


if __name__ == "__main__":
    main(sys.argv[1])
