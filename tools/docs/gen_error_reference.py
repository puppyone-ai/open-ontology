#!/usr/bin/env python3
"""Generate the searchable L1 error-code index; keep operator prose intact."""
import argparse
from pathlib import Path
import re

from gen_cli_reference import ROOT, write_or_check
from gen_dba_grants import production_source

MARKER = "<!-- generated error index -->"
PATHS = (
    "crates/oo-cli/src", "crates/oo-gateway/src", "crates/oo-sdk/src",
    "crates/oo-stack/src", "tools/oo-load/src", "tools/model-egress",
    "tools/pg-builder/scripts", "tools/release-coordinator/src", "deploy/lite",
    "packages/cli/bin", "crates/oo-pipeline/migrations",
)
CODE = re.compile(r"""["']((?:[A-Z][A-Z0-9]*_)+[A-Z0-9_]+|OO0[0-9]{2})(?=["':\s])""")
LOWER = re.compile(
    r'(?:Failure::\w+|SourceError::\w+|err|refuse|bail|error)\(\s*"([a-z]+_[a-z_]+)"')
SPECIAL = ("review_required", "publication_unverified", "budget_unmeasured",
           "source_time_unmeasured")


def codes(source):
    source = production_source(source)
    found = [(m[1], source.count("\n", 0, m.start()) + 1)
             for pattern in (CODE, LOWER) for m in pattern.finditer(source)]
    for code in SPECIAL:
        match = re.search(r"""["']""" + code + r"""["']""", source)
        if match:
            found.append((code, source.count("\n", 0, match.start()) + 1))
    return found


def route(code):
    if code.startswith("OO0"):
        return "database-proof"
    if code == "review_required" or any(x in code for x in ("REVIEW", "review", "PUBLICATION", "publication", "REGISTRY", "FENCE")):
        return "review-and-publication"
    if any(x in code for x in ("MODEL", "UPSTREAM", "BUDGET", "EGRESS", "budget")):
        return "model"
    if any(x in code for x in ("SECRET", "CREDENTIAL", "TOKEN", "AUTH", "LOGIN", "ENROLL", "PROFILE", "CONFIG_PERMISSIONS")):
        return "credentials"
    if any(x in code for x in ("TLS", "CERT", "CA_", "NETWORK", "CONNECT", "DNS")):
        return "connectivity"
    if any(x in code for x in ("LITE", "REMOTE", "SSH", "BUNDLE", "NATIVE", "PLATFORM")):
        return "installation-and-runtime"
    if any(x in code for x in ("TIMEOUT", "OVERLOAD", "RATE_LIMIT", "CANCEL", "BUSY")):
        return "query-budget"
    return "configuration-and-source"


def render():
    entries = {}
    for relative in PATHS:
        for path in sorted((ROOT / relative).rglob("*")):
            if path.suffix not in (".rs", ".py", ".mjs", ".sql"):
                continue
            if path.name.startswith("test_") or "tests" in path.parts:
                continue
            for code, line in codes(path.read_text()):
                entries.setdefault(code, (path.relative_to(ROOT), line))
    if not {"UPSTREAM_TIMEOUT", "review_required", "OO030"} <= entries.keys():
        raise ValueError("L1 error inventory lost required sources")
    out = [
        MARKER, "", "## Error-code index", "",
        "Generated from CLI, npm wrapper, Gateway, SDK, stack, loader, retained "
        "publisher, Builder, model-egress, lite and pipeline SQL sources. "
        "Includes conservative configuration/status matches as well as errors; "
        "appearance here does not mean an error was observed in a live run. "
        "Dynamic external-driver messages are not enumerable. Each remedy links "
        "to a cause/fix section; the source location identifies the precise guard.",
        "",
        "| Code | Cause and fix | First source location |",
        "| --- | --- | --- |",
    ]
    for code, (path, line) in sorted(entries.items()):
        group = route(code)
        out.append(f"| `{code}` | [{group}](#{group}) | "
                   f"[`{path.name}:{line}`](../../{path}#L{line}) |")
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    path = ROOT / "docs/guide/troubleshooting.md"
    prose = path.read_text().split(MARKER)[0].rstrip() + "\n\n"
    write_or_check(path, prose + render(), args.check)
