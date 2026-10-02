#!/usr/bin/env python3
"""Independent crates.io age/checksum/yank and OSV check for the archived lock."""
import json
import ssl
import time
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

LOCK = Path("/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a/archive/testing/spikes/p05-pg17-parser/Cargo.lock")
OUT = Path("/tmp/oo-a076-81b3f22f-b143-4cf8-b2c8-19110890d30a/out")
AS_OF = datetime(2026, 10, 1, 23, 7, 52, tzinfo=timezone.utc)
FLOOR = 604800

def parse_lock(text: str):
    packages = []
    cur = {}
    for line in text.splitlines():
        if line.startswith("[[package]]"):
            if cur:
                packages.append(cur)
            cur = {}
            continue
        if line.startswith("name = "):
            cur["name"] = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("version = ") and "version" not in cur:
            cur["version"] = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("checksum = "):
            cur["checksum"] = line.split("=", 1)[1].strip().strip('"')
        elif line.startswith("source = "):
            cur["source"] = line.split("=", 1)[1].strip().strip('"')
    if cur:
        packages.append(cur)
    return [p for p in packages if p.get("source", "").startswith("registry+")]

def get(url: str):
    req = urllib.request.Request(url, headers={"User-Agent": "oo-a076-review (checksum check)", "Accept": "application/json"})
    with urllib.request.urlopen(req, timeout=60) as resp:
        return json.load(resp)

def main():
    pkgs = parse_lock(LOCK.read_text())
    rows = []
    mismatches = young = yanked = missing_checksum = 0
    for pkg in pkgs:
        url = f"https://crates.io/api/v1/crates/{pkg['name']}/{pkg['version']}"
        try:
            data = get(url)
        except Exception as exc:
            rows.append({"name": pkg["name"], "version": pkg["version"], "error": str(exc)})
            continue
        ver = data["version"]
        created = datetime.fromisoformat(ver["created_at"].replace("Z", "+00:00"))
        age = (AS_OF - created).total_seconds()
        checksum = ver.get("checksum")
        lock_sum = pkg.get("checksum")
        match = checksum == lock_sum
        is_yanked = bool(ver.get("yanked"))
        is_young = age < FLOOR
        if not match:
            mismatches += 1
        if is_young:
            young += 1
        if is_yanked:
            yanked += 1
        if not lock_sum:
            missing_checksum += 1
        rows.append({
            "name": pkg["name"],
            "version": pkg["version"],
            "created_at": ver["created_at"],
            "age_s": int(age),
            "yanked": is_yanked,
            "young": is_young,
            "checksum_match": match,
            "lock_checksum": lock_sum,
            "crates_checksum": checksum,
            "license": ver.get("license"),
            "rust_version": ver.get("rust_version"),
        })
        time.sleep(0.05)
    queries = [{"package": {"name": r["name"], "ecosystem": "crates.io"}, "version": r["version"]} for r in rows if "error" not in r]
    req = urllib.request.Request(
        "https://api.osv.dev/v1/querybatch",
        data=json.dumps({"queries": queries}).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "oo-a076-review"},
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        osv = json.load(resp)
    hits = []
    for pkg, result in zip([r for r in rows if "error" not in r], osv.get("results", [])):
        vulns = result.get("vulns") or []
        if vulns:
            hits.append({"name": pkg["name"], "version": pkg["version"], "ids": [v.get("id") for v in vulns]})
    summary = {
        "as_of": AS_OF.isoformat(),
        "floor_s": FLOOR,
        "registry_packages": len(pkgs),
        "checked": len(rows),
        "errors": sum(1 for r in rows if "error" in r),
        "checksum_mismatches": mismatches,
        "young": young,
        "yanked": yanked,
        "missing_lock_checksum": missing_checksum,
        "osv_hits": len(hits),
        "osv_hit_detail": hits,
    }
    (OUT / "supply-rows.json").write_text(json.dumps(rows, indent=2) + "\n")
    (OUT / "supply-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    (OUT / "osv-batch.json").write_text(json.dumps(osv) + "\n")
    print(json.dumps(summary, indent=2))

if __name__ == "__main__":
    main()
