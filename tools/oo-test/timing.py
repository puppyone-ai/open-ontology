"""Exclusive stage timings; absent measurements are never reported as zero."""
import argparse
from contextlib import contextmanager
from contextvars import ContextVar
from functools import wraps
import json
from pathlib import Path
import threading
import time

STAGES = ("container_start", "seed", "product_run", "compare", "cleanup")


class Timing:
    def __init__(self, run, clock=time.monotonic):
        self.run = run
        self.clock = clock
        self.started = clock()
        self.events = []
        self.stack = ContextVar("timing_stack", default=())
        self.lock = threading.RLock()

    def background(self, name, started, *, raised=False):
        """Record a subprocess lifetime without borrowing the caller's stack."""
        if name not in STAGES:
            raise ValueError("unknown timing stage")
        elapsed = self.clock() - started
        with self.lock:
            self.events.append({
                "stage": name, "offset_seconds": started - self.started,
                "elapsed_seconds": elapsed, "exclusive_seconds": elapsed,
                "raised": raised, "background": True,
            })
            self.persist()

    @contextmanager
    def stage(self, name):
        if name not in STAGES:
            raise ValueError("unknown timing stage")
        parent = self.stack.get()
        frame = {"stage": name, "start": self.clock(), "children": 0.0}
        token = self.stack.set((*parent, frame))
        failed = False
        try:
            yield
        except BaseException:
            failed = True
            raise
        finally:
            elapsed = self.clock() - frame["start"]
            self.stack.reset(token)
            if parent:
                parent[-1]["children"] += elapsed
            with self.lock:
                self.events.append({
                    "stage": name,
                    "offset_seconds": frame["start"] - self.started,
                    "elapsed_seconds": elapsed,
                    "exclusive_seconds": max(0.0, elapsed - frame["children"]),
                    "raised": failed,
                })
                self.persist()

    def snapshot(self):
        return {
            "format": "oo-test-timing/v1",
            "wall_seconds": self.clock() - self.started,
            "stages": {
                name: {
                    "seconds": sum(e["exclusive_seconds"] for e in self.events
                                   if e["stage"] == name)
                    if any(e["stage"] == name for e in self.events) else None,
                    "calls": sum(e["stage"] == name for e in self.events),
                    "raised": sum(e["stage"] == name and e["raised"]
                                  for e in self.events),
                } for name in STAGES
            },
            "events": list(self.events),
            "note": "Exclusive within a call stack; concurrent work is not wall time. "
                    "Background lifetimes may overlap foreground stages. "
                    "raised records exceptions, not suite verdicts.",
        }

    def persist(self):
        value = self.snapshot()
        path = self.run.path
        (path / "timing.json").write_text(json.dumps(value, indent=2) + "\n")
        receipt = path / "resource-receipt.json"
        if receipt.exists():
            data = json.loads(receipt.read_text())
            data["timing"] = value
            receipt.write_text(json.dumps(data, indent=2) + "\n")
        if isinstance(getattr(self.run, "receipt", None), dict):
            self.run.receipt["timing"] = value


def timed(name):
    """The first argument is a Run, or a harness/target with a .run."""
    def decorate(function):
        @wraps(function)
        def wrapped(owner, *args, **kwargs):
            run = getattr(owner, "run", owner)
            timer = getattr(run, "timing", None)
            # Test doubles and independently constructed hook contexts may lack it.
            if timer is None:
                return function(owner, *args, **kwargs)
            with timer.stage(name):
                return function(owner, *args, **kwargs)
        return wrapped
    return decorate


def product_command(function):
    @wraps(function)
    def wrapped(run, command, *args, **kwargs):
        if command and Path(command[0]).name in (
                "oo-load", "oo", "oo-cli", "oo-builder", "release-coordinator"):
            return timed("product_run")(function)(run, command, *args, **kwargs)
        return function(run, command, *args, **kwargs)
    return wrapped


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[2] / "evidence"
    path = args.evidence.resolve()
    if not path.is_relative_to(root.resolve()) or not path.is_dir():
        parser.error("expected a directory inside this worktree's evidence/")
    rows = []
    for receipt in sorted(path.rglob("resource-receipt.json")):
        if not receipt.resolve().is_relative_to(root.resolve()):
            parser.error("receipt outside this worktree's evidence/")
        data = json.loads(receipt.read_text())
        timing = data.get("timing")
        rows.append({"receipt": str(receipt.relative_to(root)),
                     "timing": timing})
    # Timing-only contexts include failures before a resource receipt exists.
    known = {r["receipt"].removesuffix("resource-receipt.json") for r in rows}
    for receipt in sorted(path.rglob("timing.json")):
        if not receipt.resolve().is_relative_to(root.resolve()):
            parser.error("receipt outside this worktree's evidence/")
        relative = str(receipt.relative_to(root))
        if relative.removesuffix("timing.json") not in known:
            rows.append({"receipt": relative, "timing": json.loads(receipt.read_text())})
    for row in rows:
        timing = row["timing"]
        if timing is not None and timing.get("format") != "oo-test-timing/v1":
            parser.error("unsupported timing receipt")
    if args.json:
        print(json.dumps({"receipts": rows}, indent=2))
    else:
        print("receipt\t" + "\t".join(STAGES) + "\twall_seconds")
        for row in rows:
            timing = row["timing"]
            values = ([timing["stages"][s]["seconds"] for s in STAGES] +
                      [timing["wall_seconds"]]) if timing else [None] * 6
            print(row["receipt"] + "\t" + "\t".join(
                "unmeasured" if v is None else f"{v:.3f}" for v in values))
    return 0 if rows and all(row["timing"] for row in rows) else 1
