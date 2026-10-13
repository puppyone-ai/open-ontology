#!/usr/bin/python3
"""Emit the already sealed PostgreSQL transaction only for its reserved job pin."""
import hashlib
import json
from pathlib import Path
import sys


INPUT = Path("/work-out/native-full-20261013-t-v2/transaction-803-producer.json")
INPUT_SHA = "0a0cc0d3042014debd074f364cf8df8e698c1121032819e68938264b4d3946c7"
BINDING = "3dd3e1e9-4085-4b00-bc44-aed278a3cf1f"


def main():
    context = json.load(sys.stdin)
    if (context.get("protocol") != "oo.jobs.native-producer/v1"
            or context.get("binding") != BINDING
            or context.get("pin") != 1
            or context.get("method") != "C1"
            or context.get("cycle") != "pull"
            or context.get("source_id") != "oo_dl01w1_46"):
        raise ValueError("worker binding/pin context")
    data = INPUT.read_bytes()
    if hashlib.sha256(data).hexdigest() != INPUT_SHA:
        raise ValueError("frozen source producer changed")
    output = json.loads(data)
    if output["binding"] != BINDING or output["pin"] != 1:
        raise ValueError("frozen producer identity")
    sys.stdout.buffer.write(data)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, TypeError) as error:
        print(f"pg46 transaction producer: {error}", file=sys.stderr)
        raise SystemExit(2)
