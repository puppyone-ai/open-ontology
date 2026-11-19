"""LongMemEval-V2 memory backend that drives the Open Ontology engine.

`insert` streams each trajectory to the `lme_v2_memory` engine process
(crates/oo-docs/examples/lme_v2_memory.rs): markdown rendering, oo.doc-ir/v1
parsing and normalization, rules/1 extraction and the bm25/1 lexical channel.
`query` asks the engine for evidence under a token budget and returns it as
text memory items. The engine never sees question ids, types or answers, and
it returns observations, never answers.

The engine binary comes from the OO_LME_ENGINE environment variable, so no
local path ends up in the saved memory config.
"""

from __future__ import annotations

import json
import os
import subprocess
import threading

from memory_modules.memory import Memory, MemoryContextItem, register_memory


@register_memory
class OntologyMemory(Memory):
    memory_type = "oo"

    def __init__(self, memory_params: dict[str, object]) -> None:
        super().__init__(memory_params)
        self._budget = int(memory_params.get("budget_tokens", 24000))
        self._digest_share = int(memory_params.get("digest_share_pct", 20))
        self._query_options = {
            k: memory_params[k]
            for k in (
                "per_page",
                "full_top",
                "phrase_boost",
                "coverage_note",
                "phrase_counts",
                "compact",
                "agg_sum",
            )
            if k in memory_params
        }
        engine_args: list[str] = []
        for k in ("window", "spread"):
            if k in memory_params:
                engine_args += [f"--{k}", str(int(memory_params[k]))]
        self._lock = threading.Lock()
        engine = os.environ.get("OO_LME_ENGINE")
        if not engine:
            raise RuntimeError("set OO_LME_ENGINE to the lme_v2_memory engine binary")
        self._proc = subprocess.Popen(
            [engine, *engine_args],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )

    def _call(self, request: dict[str, object]) -> dict[str, object]:
        with self._lock:
            assert self._proc.stdin is not None and self._proc.stdout is not None
            self._proc.stdin.write(json.dumps(request, ensure_ascii=False) + "\n")
            self._proc.stdin.flush()
            line = self._proc.stdout.readline()
        if not line:
            raise RuntimeError("engine process exited")
        reply = json.loads(line)
        if not reply.get("ok"):
            raise RuntimeError(f"engine error: {reply.get('error')}")
        return reply

    def insert(self, trajectory: dict[str, object]) -> None:
        self._call({"op": "insert", "trajectory": trajectory})

    def query(self, query: str, query_image: str | None = None) -> list[MemoryContextItem]:
        reply = self._call(
            {
                "op": "query",
                "text": query,
                "budget": self._budget,
                "digest_share": self._digest_share,
                **self._query_options,
            }
        )
        return [{"type": "text", "value": v} for v in reply["items"] if v.strip()]

    def engine_stats(self) -> dict[str, object]:
        return self._call({"op": "stats"})
