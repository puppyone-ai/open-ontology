# Findings

Goal remains open after the minimum gate. Live `native-state` has `enabled` false at generation 6. A read-only C5 cycle after that lease produced the same Arrow digest as generation 6, with `reader_mutations` 0 and `fullchain_executed` false. No worker process is observing the 1000 ms period. `source_write_attempts` 0 fits that read-only capture and does not prove ongoing reconcile.

`pg_default_acl` in `oo_os_connection_20261014` grants no future table SELECT to `oo_r_ledger`. A generation past 6 would be a new physical table. Manual GRANT is out of scope. Root owns the mixed per-table ACL compiler output before this source reuses it. Until that output exists, this source does not fire `native-step` and does not open generation 7.

The global `engines.json` digest moved from the resume identity `51308e7a…` to `358c6c75…` while the opensearch row stayed `whole: true`. That resume drift is the shared registry bug root already holds. `onboard.py` was not edited. The old run was not resumed again and its journal pins were not rewritten.

Declared capture is C5, consistency B, `delete_detection` reconcile, PIT `snapshot_token` false. The ontology file does not claim a change stream. `STALE_DATA` and `awaiting_publish` stay as stored. They are not a formal freshness certificate.



Attempt 4 of run `4458418024444808b4bf3731b53b6cb7` reached overlay with every numbered HTTP exit 0, copied the generation-6 maintenance files, then exited 2. `export_oos.py` creates `product/oos` with `mkdir(exist_ok=False)`. The generation-5 `oos` directory from 14:24 was still there, so the call raised `FileExistsError`. Shared `product_flow.py` maps that to the generic refusal. A clean out directory exports the same `effective.json` successfully. The generation-5 tree was moved to `oos-g5-retained` and left intact. Attempt 5 then exported a new `product/oos` and wrote `product-receipt.json`.

`phase_floor` uses the run directory birth time only when this run's recovery verdict is verified, generation 6, and `source_write_attempts` is 0. A run without that verdict keeps the process-start floor. The unit test covers both.

A completed dialogue transcript is rechecked by launching `dialogue.py` with `resume_transcript`. The shell command keeps the `dialogue.py` path visible for 1.2 seconds so the onboard 0.5 second process sample records it. Attempt 5 recorded both programs. The recheck exited 0 with the same three query ids and an empty stderr.

Earlier failures stay on the run: `cancel-schema-invalid` (CLI exit 2, `SCHEMA_INVALID`, policy catalog unavailable, empty operations), `dialogue-gpt-failed`, `dialogue-grok-fenced`, `dialogue-grok-shape`, `dialogue-grok-timeout`, and whole attempts 0–4 exit 2. They were not rewritten as success.

The generation-6 catalog installed on the existing gateway is collection `b688c0e02b3809d7d959f478aa87a78e8bb10b9751d6455e25bf870bfa04fe60`, file SHA-256 `89fcdf8e3e02e0139257b693304033d071ee9a619be6367ad945540ae7f48a86`, pack digest `31b87ce91313a3378bc9d7355221da4e54708fc78d530d4139822dc4d0f11300`. Schema digest remains `9aac7b730f0a2df7fe6487fee9da336428ce780ce2bf6954c9595821c51ba8c6`. Maintenance `llm_calls` is 0. The generation-5 pack was not copied onto generation 6.

Linux rustc 1.94 failed to compile this crate because `TargetReport` now requires `capture_witness` and `CaptureMethod` now requires `delete_detection`. Both are optional and omitted from legacy JSON when unset. This crate now sets both to `None`. Shared `source-admission` was not edited. The same `cargo test --locked` then passed.

`~/.cache/oo-grok-connection-full-20261014/ROOT-OBSERVATIONS.json` still names run `a223a74bb93241f1b6ff5d771570eed2`. `INFRA-STATE.json` `product_pass` is false at `2026-10-14T05:44:28Z`. Those files are outside this write domain and were left as they are. ClickHouse, StarRocks, Oracle, and PostgreSQL engine-registry records were unchanged by the opensearch register. PostgreSQL's awaiting record was already present when this register ran.
