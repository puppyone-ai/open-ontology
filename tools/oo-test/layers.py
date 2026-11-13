"""Layer planning for `oo-test fast` (plan sections 1, 2.4, 2.6, 2.7).

- Version equivalence classes come from the connectors' own version gates:
  every threshold comparison in the version-dispatch files is extracted, and
  two versions share a class for a suite only when every gate that touches
  that suite's capabilities evaluates the same for both.
- Each class runs fully on one representative, rotated daily; the other
  members run a capability probe. Security suites are never sampled.
- Impact selection maps changed files to engine cells from coverage maps plus
  manual rules; anything unmapped selects everything.
- The scheduler packs cells into waves under a memory budget and the host's
  container caps.
"""
import datetime
import fnmatch
import json
from pathlib import Path
import re
import subprocess

import runner

P0 = {"postgres": ["13", "14", "15", "16", "17", "18"],
      "mysql": ["5.7", "8.0", "8.4", "9"],
      "oracle": ["11", "19", "21", "23"]}

# Capabilities each suite exercises. "security" is never sampled (spec section 6).
SUITE_CAPS = {"T0": {"catalog"}, "T1": {"snapshot", "catalog"}, "T8": {"snapshot"},
              "T2": {"cdc", "catalog"}, "T4": {"cdc", "catalog"}, "T7": {"security"}}
ALL = {"catalog", "snapshot", "cdc", "security"}

GATE_FILES = {
    "postgres": ["crates/oo-connectors/src/postgres/version.rs",
                 "crates/oo-connectors/src/postgres/changes.rs",
                 "crates/oo-connectors/src/postgres/probe.rs"],
    "mysql": ["crates/oo-connectors/src/mysql/capability.rs",
              "crates/oo-connectors/src/mysql/version.rs"],
    "oracle": ["crates/oo-connectors/src/oracle/rules.rs"],
}
# Named gates whose scope is narrower than "everything". Unknown names are ALL.
PG_FUNCTIONS = {"tid_range_scan": {"snapshot"}, "snapshot_function": {"snapshot"},
                "virtual_generated_columns": {"catalog", "snapshot"},
                "publish_via_partition_root": {"cdc"}, "logical_decoding_on_standby": {"cdc"},
                "failover_slots": {"cdc"}, "idle_slot_timeout": {"cdc"},
                "pgoutput_protocol": {"cdc"}}
ORACLE_FIELDS = {"fetch_first": {"snapshot", "catalog"}, "standard_hash": {"snapshot"},
                 "column_collation": {"catalog", "snapshot"},
                 "identity_columns": {"catalog"}, "continuous_mine": {"cdc"},
                 "logmining_privilege": {"cdc"}, "logminer_excluded_tablespaces": {"cdc"}}
FILE_CAPS = {"changes.rs": {"cdc"}, "version.rs:mysql": {"cdc"}}

MEMORY_GB = {"oracle": 4, "mysql": 2, "postgres": 1}
TARGET_GB = 1  # each product cell also starts one PostgreSQL Agent DB


def tuple_of(text):
    return tuple(int(x) for x in re.findall(r"\d+", text)[:3]) + (0,) * (3 - len(re.findall(r"\d+", text)[:3]))


def gates(root=runner.ROOT):
    """Every version threshold the connectors act on, with its capability scope."""
    found = []
    for engine, files in GATE_FILES.items():
        for name in files:
            text = (root / name).read_text()
            base = Path(name).name
            if engine == "postgres":
                function = None
                for line in text.splitlines():
                    match = re.search(r"\bfn (\w+)", line)
                    if match:
                        function = match[1]
                    if re.search(r"#\[(test|cfg\(test\))\]", line):
                        break  # thresholds asserted in tests are not dispatch
                    for op, value in re.findall(r"major\s*(>=|<|>|<=)\s*(\d+)", line):
                        caps = (FILE_CAPS.get(base) or PG_FUNCTIONS.get(function) or ALL)
                        found.append({"engine": engine, "file": name, "gate": function,
                                      "threshold": (int(value) + (op in (">", "<=")), 0, 0),
                                      "caps": sorted(caps)})
                    arm = re.match(r"\s*(\d{2}) => \d+,", line)
                    if arm:
                        found.append({"engine": engine, "file": name, "gate": "pgoutput_protocol",
                                      "threshold": (int(arm[1]), 0, 0), "caps": ["cdc"]})
            elif engine == "mysql":
                body = text.split("#[cfg(test)]")[0]
                caps = FILE_CAPS.get(base + ":mysql") or ALL
                for value in re.findall(r"\((\d+), (\d+), (\d+)\)", body):
                    found.append({"engine": engine, "file": name, "gate": "version_tuple",
                                  "threshold": tuple(map(int, value)), "caps": sorted(caps)})
                for major, minor in re.findall(r"\((\d+), (\d+)\)\s*\|", body):
                    found.append({"engine": engine, "file": name, "gate": "major_minor",
                                  "threshold": (int(major), int(minor), 0), "caps": sorted(ALL)})
            else:
                body = text.split("#[cfg(test)]")[0]
                field = None
                for line in body.splitlines():
                    match = re.match(r"\s*([a-z_]\w*):(?!:)", line)
                    if match:
                        field = match[1]
                    for major, minor in re.findall(r"at_least\((\d+), (\d+)\)", line):
                        caps = ORACLE_FIELDS.get(field, ALL)
                        found.append({"engine": engine, "file": name, "gate": field,
                                      "threshold": (int(major), int(minor), 0),
                                      "caps": sorted(caps)})
    return found


def actual(engine, version):
    """The exact locked version (e.g. 9 -> 9.5.0, 23 -> 23.26.x)."""
    return tuple_of(re.search(r"\d+(?:\.\d+)*", runner.resolve(engine, version)["id"]).group())


def classes(engine, suite, gate_list=None, versions=None):
    versions = versions or P0[engine]
    caps = SUITE_CAPS[suite]
    if "security" in caps:
        return [[v] for v in versions]
    relevant = [g for g in (gate_list or gates()) if g["engine"] == engine
                and set(g["caps"]) & caps]
    groups = {}
    for version in versions:
        exact = actual(engine, version)
        signature = tuple(exact >= tuple(g["threshold"]) for g in relevant)
        groups.setdefault(signature, []).append(version)
    return list(groups.values())


def representative(members, day):
    """Daily rotation: over len(members) days every member is the representative once."""
    return members[day.toordinal() % len(members)]


def l1_cells(engines, suites, day, gate_list=None):
    """Per version: the suites it runs fully, and the ones it only probes."""
    gate_list = gate_list or gates()
    cells = []
    for engine in engines:
        plan = {v: {"full": [], "probe": []} for v in P0[engine]}
        for suite in suites:
            for members in classes(engine, suite, gate_list):
                rep = representative(members, day)
                for version in members:
                    plan[version]["full" if version == rep else "probe"].append(suite)
        for version, value in plan.items():
            cells.append({"engine": engine, "version": version, **value})
    return cells


# -- impact selection ---------------------------------------------------------
MANUAL_RULES = [
    ("crates/oo-connectors/src/oracle/**", {"oracle"}),
    ("crates/oo-connectors/src/postgres/**", {"postgres"}),
    ("crates/oo-connectors/src/mysql/**", {"mysql"}),
    ("testing/engines/oracle/**", {"oracle"}),
    ("testing/engines/postgres/**", {"postgres"}),
    ("testing/engines/mysql/**", {"mysql"}),
    ("crates/oo-connectors/src/engine/**", {"postgres", "mysql", "oracle"}),
    ("tools/oo-load/src/**", {"postgres", "mysql", "oracle"}),
    ("tools/oo-test/**", {"postgres", "mysql", "oracle"}),
    ("testing/engines/seed-common/**", {"postgres", "mysql", "oracle"}),
]
NO_CELLS = ["docs/**", "*.md", "execution/**", "specs/**", "apps/web/**", ".github/**"]


def impacted(changed, coverage_maps=None):
    """Engines whose L1 cells a change can affect; unknown product code selects all."""
    coverage_maps = coverage_maps or {}
    selected, reasons = set(), {}
    for name in changed:
        engines = None
        for pattern, value in MANUAL_RULES:
            if fnmatch.fnmatch(name, pattern):
                engines = set(value)
                reasons[name] = "rule:" + pattern
                break
        if engines is None:
            covered = {engine for engine, files in coverage_maps.items() if name in files}
            if covered:
                engines = covered
                reasons[name] = "coverage"
        if engines is None and any(fnmatch.fnmatch(name, p) for p in NO_CELLS):
            engines = set()
            reasons[name] = "no_cells"
        if engines is None:
            engines = set(P0)
            reasons[name] = "unmapped_selects_all"
        selected |= engines
    return selected, reasons


def changed_files(base):
    out = subprocess.check_output(["git", "diff", "--name-only", base + "...HEAD"],
                                  cwd=runner.ROOT, text=True)
    return [line for line in out.splitlines() if line]


def coverage_map(export_path):
    """Product files a coverage export reports as executed at least once."""
    data = json.loads(Path(export_path).read_text())
    files = set()
    for entry in data["data"]:
        for item in entry["files"]:
            if item["summary"]["lines"]["covered"]:
                try:
                    files.add(str(Path(item["filename"]).resolve().relative_to(runner.ROOT.resolve())))
                except ValueError:
                    pass
    return files


# -- memory-aware scheduler -----------------------------------------------------
def cell_memory(engine):
    return MEMORY_GB[engine] + TARGET_GB


def schedule(cells, budget_gb, max_oracle=1, max_other_containers=2):
    """First-fit-decreasing waves: memory, one Oracle, and the non-Oracle container cap.

    Every product cell uses its source plus one PostgreSQL target; the target
    counts against the non-Oracle cap.
    """
    pending = sorted(cells, key=lambda c: -cell_memory(c["engine"]))
    for cell in pending:
        if cell_memory(cell["engine"]) > budget_gb:
            raise ValueError("cell_exceeds_memory_budget")
        other = 1 if cell["engine"] == "oracle" else 2
        if other > max_other_containers:
            raise ValueError("cell_exceeds_container_cap")
    waves = []
    while pending:
        wave, memory, oracle, other = [], 0, 0, 0
        for cell in list(pending):
            need = cell_memory(cell["engine"])
            is_oracle = cell["engine"] == "oracle"
            extra = 1 if is_oracle else 2
            if (memory + need <= budget_gb and oracle + is_oracle <= max_oracle
                    and other + extra <= max_other_containers):
                wave.append(cell)
                pending.remove(cell)
                memory, oracle, other = memory + need, oracle + is_oracle, other + extra
        waves.append({"cells": wave, "memory_gb": memory})
    return waves


def docker_budget_gb(cap_gb=40, fraction=0.7):
    total = int(subprocess.check_output(["docker", "info", "--format", "{{.MemTotal}}"],
                                        text=True).strip())
    return min(cap_gb, fraction * total / 1024 ** 3)


def today():
    return datetime.date.today()
