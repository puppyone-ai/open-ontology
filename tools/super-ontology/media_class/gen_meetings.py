#!/usr/bin/env python3
"""Multi-voice synthetic meetings with a reference timeline (D98 re-validation, lane AL). macOS `say` and `ffmpeg` only.

    python3 gen_meetings.py OUT_DIR

Writes `<name>.wav` (16 kHz mono) and `<name>.ref.json` (`{speakers, segments: [{speaker, start_ms, end_ms}]}`) for two and three
speakers in Chinese and in English. Every speaker has its own macOS voice, speaking rate and pitch base.
"""
import json
import subprocess
import sys
import tempfile
from pathlib import Path

ZH = {
    "zh2": [("Tingting (中文（中国大陆）)", 170, 50), ("Grandpa (中文（中国大陆）)", 200, 30)],
    "zh3": [("Tingting (中文（中国大陆）)", 170, 50), ("Grandpa (中文（中国大陆）)", 200, 30), ("Rocko (中文（中国大陆）)", 150, 45)],
    "zh2b": [("Flo (中文（中国大陆）)", 185, 55), ("Reed (中文（中国大陆）)", 160, 35)],
    "zh3b": [("Sandy (中文（中国大陆）)", 175, 50), ("Shelley (中文（中国大陆）)", 195, 60), ("Eddy (中文（中国大陆）)", 155, 30)],
}
EN = {
    "en2": [("Daniel (英语（英国）)", 170, 40), ("Karen", 190, 55)],
    "en3": [("Daniel (英语（英国）)", 170, 40), ("Karen", 190, 55), ("Ralph", 150, 30)],
    "en2b": [("Fred", 180, 35), ("Kathy", 165, 55)],
    "en3b": [("Albert", 175, 40), ("Junior", 195, 60), ("Kathy", 155, 50)],
}
TEXT_ZH = [
    "大家好，我们开始今天的项目周会，先看一下上周的进度。", "好的，原型已经完成，下周一开始内部测试，需要三位同事参与。",
    "测试环境谁来搭建？预算方面还有没有问题？", "环境我来搭建，预算够用，不需要追加。",
    "那我补充一点，供应商的报价已经确认，交付周期是二十一天。", "很好，那就定在周五之前把测试计划发给所有人。",
    "收到，我周四晚上发出来，下周二再开一次短会同步。",
]
TEXT_EN = [
    "Good morning everyone, let's start the weekly project meeting with last week's progress.", "The prototype is finished and internal testing starts on Monday, we need three people.",
    "Who will set up the test environment, and is the budget still fine?", "I will set up the environment, and the budget is enough, no extra money is needed.",
    "One more point, the supplier quote is confirmed and delivery takes twenty one days.", "Great, then we send the test plan to everyone before Friday.",
    "Understood, I will send it Thursday evening and we meet again next Tuesday.",
]


def run(*cmd, capture=False):
    return subprocess.run(cmd, check=True, capture_output=capture, text=True)


def duration_ms(path):
    out = run("ffprobe", "-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0", str(path), capture=True).stdout
    return int(float(out) * 1000)


def make(name, voices, texts, out):
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        gap_ms = 450
        run("ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", "anullsrc=r=16000:cl=mono", "-t", str(gap_ms / 1000), str(tmp / "gap.wav"))
        parts, segments, cursor = [], [], 0
        for i, text in enumerate(texts):
            k = i % len(voices)
            voice, rate, pitch = voices[k]
            run("say", "-v", voice, "-r", str(rate), "-o", str(tmp / f"s{i}.aiff"), f"[[pbas {pitch}]] {text}")
            run("ffmpeg", "-y", "-loglevel", "error", "-i", str(tmp / f"s{i}.aiff"), "-ar", "16000", "-ac", "1", str(tmp / f"s{i}.wav"))
            d = duration_ms(tmp / f"s{i}.wav")
            segments.append({"speaker": f"S{k + 1}", "start_ms": cursor, "end_ms": cursor + d})
            cursor += d + gap_ms
            parts += [tmp / f"s{i}.wav", tmp / "gap.wav"]
        (tmp / "list.txt").write_text("".join(f"file '{p}'\n" for p in parts))
        run("ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(tmp / "list.txt"), "-ar", "16000", "-ac", "1", str(out / f"{name}.wav"))
    (out / f"{name}.ref.json").write_text(json.dumps({"speakers": len(voices), "segments": segments}, indent=1))


def main(out_dir):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    for name, voices in ZH.items():
        make(name, voices, TEXT_ZH, out)
    for name, voices in EN.items():
        make(name, voices, TEXT_EN, out)
    print(sorted(p.name for p in out.glob("*.wav")))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "meetings")
