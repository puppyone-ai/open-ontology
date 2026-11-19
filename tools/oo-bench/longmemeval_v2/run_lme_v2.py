#!/usr/bin/env python3
"""Run the official LongMemEval-V2 harness with the OO memory backend.

The official repository is used unmodified (pass its checkout with
--lme-repo). This wrapper only registers the `oo` memory type, writes the
runtime inputs the way `evaluation/run_eval.py` does, and calls
`evaluation.harness.main()` with the paper's reader settings.

Model endpoints are never passed on the command line, so they never reach
run_args.json: the reader is a local OpenAI-compatible server (--reader-url,
a loopback address), and the judge endpoint and key are read from the files
named by --judge-endpoint-file / --judge-key-file into the OPENAI_BASE_URL /
OPENAI_API_KEY environment of this process only.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path


def dev_ids(questions_path: Path, domain: str, split: str) -> list[str]:
    out = []
    for line in questions_path.read_text(encoding="utf-8").splitlines():
        q = json.loads(line)
        if q["domain"] != domain:
            continue
        is_dev = hashlib.sha256(q["id"].encode()).digest()[0] < 0x56
        if split == "all" or (split == "dev") == is_dev:
            out.append(q["id"])
    return out


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--lme-repo", required=True)
    ap.add_argument("--data-root", required=True)
    ap.add_argument("--domain", choices=["web", "enterprise"], required=True)
    ap.add_argument("--tier", choices=["small", "medium"], default="small")
    ap.add_argument("--split", choices=["dev", "heldout", "all"], required=True)
    ap.add_argument("--output-dir", required=True)
    ap.add_argument("--budget-tokens", type=int, default=24000)
    ap.add_argument("--digest-share-pct", type=int, default=20)
    ap.add_argument(
        "--engine-params",
        default="{}",
        help="JSON merged into memory_params: window, spread, per_page, full_top, phrase_boost, coverage_note",
    )
    ap.add_argument("--reader-url", default="http://127.0.0.1:8023/v1")
    ap.add_argument("--reader-concurrency", type=int, default=8)
    ap.add_argument("--judge-model", default="cb/gpt-5.2")
    ap.add_argument("--judge-endpoint-file", required=True)
    ap.add_argument("--judge-key-file", required=True)
    args = ap.parse_args()

    repo = Path(args.lme_repo).resolve()
    sys.path.insert(0, str(repo))
    sys.path.insert(0, str(Path(__file__).resolve().parent))
    import oo_memory  # noqa: F401  registers memory_type "oo"
    from data.public_data import (
        materialize_runtime_haystack,
        materialize_runtime_questions,
        write_json,
    )

    os.environ["OPENAI_BASE_URL"] = Path(args.judge_endpoint_file).expanduser().read_text().strip()
    os.environ["OPENAI_API_KEY"] = Path(args.judge_key_file).expanduser().read_text().strip()
    os.environ.setdefault("READER_API_KEY", "EMPTY")

    data_root = Path(args.data_root).resolve()
    out = Path(args.output_dir).resolve()
    runtime = out / "runtime_inputs"
    runtime.mkdir(parents=True, exist_ok=True)
    ids = None if args.split == "all" else dev_ids(data_root / "questions.jsonl", args.domain, args.split)
    selected = materialize_runtime_questions(
        data_root=data_root,
        domain=args.domain,
        question_ids=ids,
        limit=None,
        output_path=runtime / "questions.json",
    )
    materialize_runtime_haystack(
        data_root=data_root,
        tier=args.tier,
        selected_questions=selected,
        output_path=runtime / "haystack.json",
    )
    config = {
        "memory_type": "oo",
        "memory_params": {
            "budget_tokens": args.budget_tokens,
            "digest_share_pct": args.digest_share_pct,
            "lexical_model": "bm25/1",
            **json.loads(args.engine_params),
        },
    }
    write_json(runtime / "memory_config.json", config)
    sys.argv = [
        "evaluation.harness",
        "--domain", args.domain,
        "--questions-path", str(runtime / "questions.json"),
        "--haystack-path", str(runtime / "haystack.json"),
        "--trajectories-path", str(data_root / "trajectories.jsonl"),
        "--memory-config-path", str(runtime / "memory_config.json"),
        "--output-dir", str(out),
        "--model", "Qwen/Qwen3.5-9B",
        "--base-url", args.reader_url,
        "--api-key-env", "READER_API_KEY",
        "--temperature", "0.6",
        "--top-p", "0.95",
        "--top-k", "20",
        "--max-completion-tokens", "20000",
        "--memory-context-max-tokens", "200000",
        "--reader-max-concurrent-requests", str(args.reader_concurrency),
        "--prompt-build-max-workers", "1",
        "--evaluator-model", args.judge_model,
        "--evaluator-api-key-env", "OPENAI_API_KEY",
        "--evaluator-reasoning-effort", "medium",
        "--evaluator-max-completion-tokens", "4096",
    ]
    import evaluation.harness as harness
    import evaluation.qa_eval_metrics as metrics

    install_transport_guards(harness, metrics)
    harness.main()


def install_transport_guards(harness, metrics) -> None:
    """Two transport-only wrappers; prompts, sampling and scoring are unchanged.

    * Reader outputs are saved to `reader_outputs.json` as they arrive, with
      the sha256 of the exact prompt messages, and reused only for a
      byte-identical prompt, so a crash in scoring never repeats hours of
      reading and a changed prompt is always read again.
    * A rate-limited or unavailable judge (HTTP 429/5xx through the proxy) is
      retried with backoff instead of failing the run.
    """
    import asyncio
    import time

    original_reader = harness.generate_all_reader_outputs

    def prompt_sha(row) -> str:
        blob = json.dumps(row["messages"], sort_keys=True, ensure_ascii=True)
        return hashlib.sha256(blob.encode()).hexdigest()

    async def cached_reader(args, prompt_rows):
        path = Path(args.output_dir) / "reader_outputs.json"
        done = json.loads(path.read_text(encoding="utf-8")) if path.exists() else {}
        # Reuse an output only for the byte-identical prompt it was read from.
        done = {
            q: o
            for q, o in done.items()
            if any(r["question_id"] == q and o.get("prompt_sha256") == prompt_sha(r) for r in prompt_rows)
        }
        todo = [r for r in prompt_rows if r["question_id"] not in done]
        lock = asyncio.Lock()
        gate = asyncio.Semaphore(args.reader_max_concurrent_requests)

        async def one(row):
            async with gate:
                out = await original_reader(args, [row])
            out[row["question_id"]]["prompt_sha256"] = prompt_sha(row)
            async with lock:
                done.update(out)
                path.write_text(json.dumps(done, ensure_ascii=True), encoding="utf-8")

        await asyncio.gather(*(one(r) for r in todo))
        return {
            r["question_id"]: {k: v for k, v in done[r["question_id"]].items() if k != "prompt_sha256"}
            for r in prompt_rows
        }

    harness.generate_all_reader_outputs = cached_reader

    original_judge = metrics._call_chat_completion

    def patient_judge(**kwargs):
        delay = 15.0
        for _ in range(400):
            try:
                return original_judge(**kwargs)
            except Exception as exc:  # noqa: BLE001
                status = getattr(exc, "status_code", None)
                if status not in (429, 500, 502, 503, 504):
                    raise
                time.sleep(delay)
                delay = min(delay * 1.5, 300.0)
        return original_judge(**kwargs)

    metrics._call_chat_completion = patient_judge


if __name__ == "__main__":
    main()
