#!/usr/bin/env python3
"""Emit a pinned producer for its normal worker; admission stays with the loader."""
import argparse
import hashlib
import json
from pathlib import Path
import sys


def emit(context, path, expected):
    data = path.read_bytes()
    if hashlib.sha256(data).hexdigest() != expected:
        raise ValueError("pinned producer changed")
    output = json.loads(data)
    if (context.get("protocol") != "oo.jobs.native-producer/v1"
            or context.get("method") != "C1" or context.get("cycle") != "pull"
            or output.get("protocol") != "oo.jobs.native-transaction-producer/v1"
            or output.get("capture_method") != "c1_cdc"
            or any(output.get(key) != context.get(key) for key in ("binding", "pin", "source_id"))
            or type(context.get("pin")) is not int or context["pin"] < 1
            or type(context.get("acknowledged_source_generation")) is not int
            or output.get("source_generation") != context["acknowledged_source_generation"] + 1
            or output.get("transaction_scope", {}).get("complete") is not True
            or not output.get("entries")):
        raise ValueError("worker context / admitted producer mismatch")
    return data


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--producer", required=True, type=Path)
    parser.add_argument("--producer-sha256", required=True)
    args = parser.parse_args()
    sys.stdout.buffer.write(emit(json.load(sys.stdin), args.producer, args.producer_sha256))


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"PostgreSQL producer refused: {error}", file=sys.stderr)
        raise SystemExit(2)
