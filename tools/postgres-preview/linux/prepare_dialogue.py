#!/usr/bin/env python3
"""Bind the PG46 autonomous dialogue to independent source gold and public CLI."""

import json
import os
from pathlib import Path

ROOT = Path("/tmp/oo-dl01w1-46-gateway")
REPO = Path(__file__).resolve().parents[3]


def main() -> None:
    source = [line.strip().split("|") for line in (ROOT / "source-gold.txt").read_text().splitlines()]
    assert len(source) == 4 and all(len(row) == 2 for row in source)
    truth = {
        key: {"expected_named_facts": {"row_count": row[0], measure: row[1]}}
        for key, measure, row in zip(
            ("notes", "hubs", "shipments", "tracking"),
            ("amount_total", "capacity_total", "cost_total", "last_event_id"),
            source,
        )
    }
    truth_path = ROOT / "dialogue-truth.json"
    truth_path.write_text(json.dumps(truth, sort_keys=True) + "\n")
    os.chmod(truth_path, 0o600)
    gateway = "http://" + (ROOT / "listen.addr").read_text().strip()
    config = {
        "model_origin": "http://127.0.0.1:8317",
        "model": "gpt-6.1-sol",
        "model_bearer_file": "/tmp/oo-product-model-20261012/bearer",
        "gateway_token_file": str(ROOT / "token.private"),
        "gateway": gateway,
        "transport": "local-development",
        "auth": "external",
        "oo_cli": str(REPO / "tools/postgres-preview/linux/gateway_cli_adapter.py"),
        "independent_truth_file": str(truth_path),
        "independent_gateway_audit_file": str(ROOT / "wal"),
        "max_tool_rounds": 6,
        "questions": [
            {
                "truth_key": "notes",
                "prompt": "Use the public OO tools only. First read the actual Pack files README.md, INDEX.md and tables/customer_east.delivery_notes.md with knowledge_cat; describe customer_east.delivery_notes and check freshness. Then query its current row count and exact total amount. Never use source access. Cite the actual query ID for each fact.",
            },
            {
                "truth_key": "hubs",
                "prompt": "Continue with public OO tools. Read tables/public.hubs.md, describe public.hubs, check freshness, then query its current row count and exact total capacity_grams. Cite actual query IDs.",
            },
            {
                "truth_key": "shipments",
                "prompt": "Continue with public OO tools. Read tables/public.shipments.md, describe public.shipments, check freshness, then query its current row count and exact total cost_amount. Preserve the complete decimal string and cite actual query IDs.",
            },
            {
                "truth_key": "tracking",
                "prompt": "Continue with public OO tools. Read tables/public.tracking_events.md, describe public.tracking_events, check freshness, then query its current row count and maximum event_id. Cite actual query IDs and do not infer from previous turns.",
            },
        ],
    }
    path = ROOT / "dialogue-config.json"
    path.write_text(json.dumps(config, sort_keys=True, indent=2) + "\n")
    os.chmod(path, 0o600)
    print(json.dumps({"config": str(path), "truth": str(truth_path), "gateway": gateway}))


if __name__ == "__main__":
    main()
