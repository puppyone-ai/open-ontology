#!/usr/bin/env python3
"""Synthetic sample Drive for the L6 CLI acceptance package (evidence/L6/ACCEPTANCE-CLI.md, step 0).

  /usr/bin/python3 tools/oo-test/super_ontology_l6_samples.py <empty directory>

Writes, all synthetic: two Markdown files, a scanned page as a png and as an image-only pdf, a 4 turn
3 speaker meeting (macOS `say` voices) as a wav, a 2 slide screen recording as webm (VP9/Opus), and, to
show what is refused without a host decoder, an H.264/AAC mp4 and an AAC m4a. Needs macOS (`say`, `sips`),
Pillow on the host python (the system python has it) and an ffmpeg with libopus, libvpx-vp9 and libx264.
The containers decode with their own FFmpeg; these tools only make the inputs.
"""
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.dont_write_bytecode = True
import super_ontology_l6_e2e as e2e  # noqa: E402

MARKDOWN = {
    "Projects/星火计划.md": "---\ntype: project\nname: 星火计划\nowner: 林川\n---\n# 星火计划\n\n负责人：林川\n\n状态：进行中\n",
    "People/林川.md": "---\ntype: person\nname: 林川\nrole: 工程负责人\n---\n# 林川\n",
}


def main(argv):
    if len(argv) != 2:
        print(__doc__)
        return 2
    root = Path(argv[1])
    if root.exists() and any(root.iterdir()):
        print(f"{root} is not empty", file=sys.stderr)
        return 2
    e2e.make_inputs(root)
    for rel, text in MARKDOWN.items():
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(text, encoding="utf-8")
    files = sorted(str(p.relative_to(root)) for p in root.rglob("*") if p.is_file())
    print("\n".join(files))
    print(f"{len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
