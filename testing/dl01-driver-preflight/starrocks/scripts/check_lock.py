#!/usr/bin/env python3
"""Fail when a generated lock drifts from the pinned MariaDB recipe lock."""

import sys


def packages(path: str) -> dict:
    found = {}
    name = None
    version = None
    checksum = None
    with open(path, encoding="utf-8") as handle:
        for line in handle:
            if line.startswith("name = "):
                if name and checksum:
                    found[(name, version)] = checksum
                name = line.split('"', 2)[1]
                version = None
                checksum = None
            elif line.startswith('version = "'):
                version = line.split('"', 2)[1]
            elif line.startswith('checksum = "'):
                checksum = line.split('"', 2)[1]
        if name and checksum:
            found[(name, version)] = checksum
    return found


def main() -> int:
    if len(sys.argv) != 3:
        print("usage: check_lock.py PIN.lock NEW.lock", file=sys.stderr)
        return 2
    pin = packages(sys.argv[1])
    new = packages(sys.argv[2])
    pin_by_name = {}
    for (name, version), checksum in pin.items():
        pin_by_name.setdefault(name, []).append((version, checksum))
    drifted = 0
    for (name, version), checksum in sorted(new.items()):
        known = pin_by_name.get(name)
        if not known:
            drifted += 1
            print(f"drift_new name={name} version={version}")
            continue
        match = [item for item in known if item[0] == version and item[1] == checksum]
        if not match:
            drifted += 1
            print(f"drift_version name={name} version={version} pin={[item[0] for item in known]}")
    print(f"lock_packages={len(new)} drift={drifted}")
    return 1 if drifted else 0


if __name__ == "__main__":
    sys.exit(main())
