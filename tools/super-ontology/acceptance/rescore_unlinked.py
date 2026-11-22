#!/usr/bin/env python3
"""Re-scores the unlinked part of a finished acceptance run from its saved files (no engine needed).

    python3 -B rescore_unlinked.py --run packaged-run.json|run-dropped.json --unlinked-ui unlinked-ui.json --entities entities.json --gold <gold dir>

`entities.json` is the file score.py wrote (names, files per entity). The mapping of app states to the gold's reasons is APP_TO_GOLD in score.py.
"""
import argparse, json, sys
from pathlib import Path

sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parent))
import score  # noqa: E402


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--run", required=True)
    ap.add_argument("--unlinked-ui", required=True)
    ap.add_argument("--entities", required=True)
    ap.add_argument("--gold", required=True)
    a = ap.parse_args()
    run = json.loads(Path(a.run).read_text())
    ui = json.loads(Path(a.unlinked_ui).read_text())
    world = [{**e, "files": set(e["files"]), "names": set(e["names"])} for e in json.loads(Path(a.entities).read_text())]
    gold = [json.loads(l) for l in (Path(a.gold) / "unlinked.jsonl").read_text().splitlines() if l]
    summary, rows = score.score_unlinked(gold, run, world, ui)
    print(json.dumps({"unlinked": summary, "rows": rows, "false_alarms": score.false_alarms(gold, run, world, ui)}, ensure_ascii=False, indent=1))


if __name__ == "__main__":
    main()
