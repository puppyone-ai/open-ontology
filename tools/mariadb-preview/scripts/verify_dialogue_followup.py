#!/usr/bin/env python3
"""Verify MariaDB's single corrective OO-tool turn against raw CLI and WAL."""

import argparse
import hashlib
import json
from pathlib import Path


def read(path):
    return json.loads(path.read_text())


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--evidence", type=Path, required=True)
    parser.add_argument("--external-denial", type=Path, required=True)
    parser.add_argument("--wal", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()

    followup = args.evidence / "dialogue-hidden-column-followup-required"
    raw = read(followup / "transcript.json")
    models = [event for event in raw if "model" in event]
    calls = [event for event in raw if "tool" in event]
    assert models and models[0]["model"]["choices"][0]["message"].get("tool_calls")
    assert len(calls) == 2
    for call in calls:
        assert call["tool"] == "validate"
        assert call["arguments"]["sql"] == "SELECT _oo_pos FROM logistics.hubs LIMIT 1"
        assert call["exit"] == 2
        assert call["response"]["class"] == "gateway"
        assert call["response"]["code"] == "COLUMN_NOT_ALLOWED"
        assert "query_id" not in call["response"]  # Rejected before admission.

    answer = models[-1]["model"]["choices"][0]["message"].get("content") or ""
    assert "COLUMN_NOT_ALLOWED" in answer and "query_id: none" in answer
    original = read(args.evidence / "dialogue-grok47-attempt" / "transcript.json")
    original_verdict = read(args.evidence / "dialogue-grok47-attempt" / "verdict.json")
    assert original_verdict["outcome"] == "verified" and original_verdict["turns"] == 4
    assert len(original_verdict["query_ids"]) == 8
    answers = [event["model"]["choices"][0]["message"].get("content") or ""
               for event in original if "model" in event
               and not event["model"]["choices"][0]["message"].get("tool_calls")]
    truth = read(args.evidence / "gateway-prep" / "independent-dialogue-truth.json")
    assert len(answers) == 4
    for answer, key in zip(answers, ("counts", "decimal", "unsigned", "history")):
        for fact in truth[key]["expected_exact_facts"]:
            assert str(fact) in answer

    completion_ids = {path.name.split("-complete.wal")[0]
                      for path in args.wal.glob("*-complete.wal")}
    assert set(original_verdict["query_ids"]) <= completion_ids
    denial = read(args.external_denial)
    assert denial["code"] == "COLUMN_NOT_ALLOWED" and denial["query_id"]

    receipt = {
        "status": "author_preview_dialogue_verified",
        "followup": "actual_OO_validate_rejected_before_query_admission",
        "followup_tool_calls": len(calls),
        "followup_code": "COLUMN_NOT_ALLOWED",
        "followup_query_id": None,
        "original_turns": 4,
        "original_query_ids_in_completion_wal": len(original_verdict["query_ids"]),
        "original_independent_gold_facts": sum(
            len(truth[key]["expected_exact_facts"])
            for key in ("counts", "decimal", "unsigned", "history")
        ),
        "external_denied_query_id": denial["query_id"],
        "raw_followup_sha256": hashlib.sha256((followup / "transcript.json").read_bytes()).hexdigest(),
        "old_strict_verdict_preserved": True,
        "formal_publication": False,
    }
    args.out.write_text(json.dumps(receipt, indent=2) + "\n")
    print(json.dumps(receipt))


if __name__ == "__main__":
    main()
