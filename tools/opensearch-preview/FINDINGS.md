# Findings

Attempt 4 of run `4458418024444808b4bf3731b53b6cb7` reached overlay with every numbered HTTP exit 0, copied the generation-6 maintenance files, then exited 2. `export_oos.py` creates `product/oos` with `mkdir(exist_ok=False)`. The generation-5 `oos` directory from 14:24 was still there, so the call raised `FileExistsError`. Shared `product_flow.py` maps that to the generic refusal. A clean out directory exports the same `effective.json` successfully. The generation-5 tree was moved to `oos-g5-retained` and left intact. Attempt 5 then exported a new `product/oos` and wrote `product-receipt.json`.

`phase_floor` uses the run directory birth time only when this run's recovery verdict is verified, generation 6, and `source_write_attempts` is 0. A run without that verdict keeps the process-start floor. The unit test covers both.

A completed dialogue transcript is rechecked by launching `dialogue.py` with `resume_transcript`. The shell command keeps the `dialogue.py` path visible for 1.2 seconds so the onboard 0.5 second process sample records it. Attempt 5 recorded both programs. The recheck exited 0 with the same three query ids and an empty stderr.

Earlier failures stay on the run: `cancel-schema-invalid` (CLI exit 2, `SCHEMA_INVALID`, policy catalog unavailable, empty operations), `dialogue-gpt-failed`, `dialogue-grok-fenced`, `dialogue-grok-shape`, `dialogue-grok-timeout`, and whole attempts 0–4 exit 2. They were not rewritten as success.

The generation-6 catalog installed on the existing gateway is collection `b688c0e02b3809d7d959f478aa87a78e8bb10b9751d6455e25bf870bfa04fe60`, file SHA-256 `89fcdf8e3e02e0139257b693304033d071ee9a619be6367ad945540ae7f48a86`, pack digest `31b87ce91313a3378bc9d7355221da4e54708fc78d530d4139822dc4d0f11300`. Schema digest remains `9aac7b730f0a2df7fe6487fee9da336428ce780ce2bf6954c9595821c51ba8c6`. Maintenance `llm_calls` is 0. The generation-5 pack was not copied onto generation 6.

Linux rustc 1.94 failed to compile this crate because `TargetReport` now requires `capture_witness` and `CaptureMethod` now requires `delete_detection`. Both are optional and omitted from legacy JSON when unset. This crate now sets both to `None`. Shared `source-admission` was not edited. The same `cargo test --locked` then passed.

`~/.cache/oo-grok-connection-full-20261014/ROOT-OBSERVATIONS.json` still names run `a223a74bb93241f1b6ff5d771570eed2`. `INFRA-STATE.json` `product_pass` is false at `2026-10-14T05:44:28Z`. Those files are outside this write domain and were left as they are. ClickHouse, StarRocks, Oracle, and PostgreSQL engine-registry records were unchanged by the opensearch register. PostgreSQL's awaiting record was already present when this register ran.
