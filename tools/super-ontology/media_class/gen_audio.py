#!/usr/bin/env python3
"""Synthetic recordings for the media class tests (D74 revision, lane AL). Stdlib, macOS `say` and `ffmpeg` only.

    python3 gen_audio.py OUT_DIR

Writes 16 kHz mono wav files: a two-speaker meeting in Chinese and in English, a music-only clip, a promo read
by one voice, the same promo over music, and a dialogue followed by music. Deterministic apart from the voices
the machine has; the classifier test runs on the pipeline output recorded from these files, not on the audio.
"""
import subprocess
import sys
import tempfile
from pathlib import Path

ZH_A, ZH_B = "Flo (中文（中国大陆）)", "Eddy (中文（中国大陆）)"
EN_A, EN_B = "Eddy (英语（美国）)", "Flo (英语（美国）)"

MEETING_ZH = [
    (ZH_A, "大家好，这里是产品周会。我是小王，今天先看一下星火计划的进度。"),
    (ZH_B, "好的，我是小李。星火计划的原型已经完成，下周开始内部测试。"),
    (ZH_A, "测试需要多少人？预算还够吗？"),
    (ZH_B, "需要三个人，预算够用。我负责测试环境的搭建。"),
    (ZH_A, "好，那就这样定了，周五之前把测试计划发给大家。"),
    (ZH_B, "收到，我会在周四晚上发出来。"),
]
MEETING_EN = [
    (EN_A, "Good morning everyone. This is the weekly product sync. I am Daniel, and I will start with the launch plan."),
    (EN_B, "Thanks Daniel. I am Priya. The prototype is ready and testing starts next week."),
    (EN_A, "How many people do we need for testing, and is the budget still fine?"),
    (EN_B, "Three people, and yes the budget is fine. I will set up the test environment myself."),
    (EN_A, "Great. Please send the test plan to everyone before Friday."),
    (EN_B, "Understood, I will send it on Thursday evening."),
]
PROMO = [
    (ZH_A, "星火智能音箱，让每一个清晨都有好心情。轻触一下，音乐、天气、日程，全部就绪。"
           "现在下单，限时立减二百元，还送一年内容会员。星火，点亮你的生活。"),
]


def run(*cmd):
    subprocess.run(cmd, check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def speech(segments, tmp, name, gap=0.35):
    parts = []
    for i, (voice, text) in enumerate(segments):
        aiff = tmp / f"{name}{i}.aiff"
        run("say", "-v", voice, "-r", "185", "-o", str(aiff), text)
        wav = tmp / f"{name}{i}.wav"
        run("ffmpeg", "-y", "-loglevel", "error", "-i", str(aiff), "-ar", "16000", "-ac", "1", str(wav))
        parts.append(wav)
    silence = tmp / "gap.wav"
    run("ffmpeg", "-y", "-loglevel", "error", "-f", "lavfi", "-i", f"anullsrc=r=16000:cl=mono", "-t", str(gap), str(silence))
    lst = tmp / f"{name}.txt"
    lst.write_text("".join(f"file '{p}'\nfile '{silence}'\n" for p in parts))
    out = tmp / f"{name}-speech.wav"
    run("ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), "-ar", "16000", "-ac", "1", str(out))
    return out


def music(tmp, seconds, name="music"):
    """A slow chord progression with a tremolo: tonal, steady, no syllable rhythm."""
    chords = [(262, 330, 392), (220, 277, 330), (175, 220, 262), (196, 247, 294)]
    each = seconds / len(chords)
    pieces = []
    for i, ch in enumerate(chords):
        out = tmp / f"{name}{i}.wav"
        inputs = []
        for f in ch:
            inputs += ["-f", "lavfi", "-i", f"sine=f={f}:r=16000:d={each}"]
        run("ffmpeg", "-y", "-loglevel", "error", *inputs, "-filter_complex",
            "amix=inputs=3:normalize=0,volume=0.5,tremolo=f=0.5:d=0.3,afade=t=in:d=0.1,afade=t=out:st=%s:d=0.2" % (each - 0.2), "-ac", "1", str(out))
        pieces.append(out)
    lst = tmp / f"{name}.txt"
    lst.write_text("".join(f"file '{p}'\n" for p in pieces))
    out = tmp / f"{name}-all.wav"
    run("ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), str(out))
    return out


def concat(parts, dest, tmp):
    lst = tmp / "cat.txt"
    lst.write_text("".join(f"file '{p}'\n" for p in parts))
    run("ffmpeg", "-y", "-loglevel", "error", "-f", "concat", "-safe", "0", "-i", str(lst), "-ar", "16000", "-ac", "1", str(dest))


def main(out_dir):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory() as t:
        tmp = Path(t)
        zh = speech(MEETING_ZH, tmp, "zh")
        en = speech(MEETING_EN, tmp, "en")
        promo = speech(PROMO, tmp, "promo")
        run("cp", str(zh), str(out / "meeting-zh.wav"))
        run("cp", str(en), str(out / "meeting-en.wav"))
        run("cp", str(promo), str(out / "promo-voice.wav"))
        run("cp", str(music(tmp, 30)), str(out / "music-only.wav"))
        # The promo read over music (the voice at full level, the music underneath).
        bed = music(tmp, 40, "bed")
        run("ffmpeg", "-y", "-loglevel", "error", "-i", str(promo), "-i", str(bed), "-filter_complex",
            "[1:a]volume=0.35[m];[0:a][m]amix=inputs=2:duration=longest:normalize=0", "-ar", "16000", "-ac", "1", str(out / "promo-over-music.wav"))
        # A short dialogue, then music: the speech is a part of the file, the floor still changes hands.
        short = speech(MEETING_EN[:4], tmp, "mix")
        concat([short, music(tmp, 25, "tail")], out / "dialogue-then-music.wav", tmp)
    for p in sorted(out.glob("*.wav")):
        print(p.name, p.stat().st_size)


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "audio")
