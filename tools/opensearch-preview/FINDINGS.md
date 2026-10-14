# Findings

Goal remains open. Two fixture I/U/D rounds on the existing binding advanced generation 6 to 12. Every capture had `reader_mutations` 0. After each delete the live document set matched pin 6 (canonical SHA-256 `c8965e1755d8e0fff0cb38599eec48ca748caa833bbd517e1362033199a5bb95`). The schedule is enabled at 1000 ms. The owned observer does not call `native-step` when the C5 probe reports no row diff; quiet ticks left generation 12 in place.

Opening those generations made the public gateway stale. `ensure_generation` compares `max(run_generation)` with the catalog binding. The maximum is 12 and the mounted catalog is still generation 6, so the public CLI now returns `SCHEMA_INVALID` / `policy catalog is not available` (`q_7eZbKDsfbIsqIvIeXfwGZQ3BxC`, `q_pkHbG7YWDiO5VC8xBVNl9XX2Z7`). `oo_r_ledger` has no SELECT on generation 12's physical table `pt_673a7926ef6d97ac56fbe19b6fcf10ce4357a5e17f30d95ea28a926d7d20`. There is still no default table SELECT. This source did not GRANT and did not point the gateway at that table. PG pid 99653 owns the mixed-generation column ACL. Until that role can read the current physical columns, this source will not publish a generation-12 gold file or rebind maintenance onto it.

The global `engines.json` digest moved from the resume identity `51308e7a…` through `358c6c75…` to `29ddf63d82d24b623683b980a722fa7cef8535769c8cfb920be4ea6274eaacd2`. The opensearch row stayed `whole: true` with the same recipe and proof. That resume drift stays with `d28ca4bd`. `onboard.py` was not edited. The old run was not resumed and its journal pins were not rewritten.

Declared capture is C5, consistency B, `delete_detection` reconcile, PIT `snapshot_token` false. The ontology file does not claim a change stream. `STALE_DATA` and `awaiting_publish` stay as stored. They are not a formal freshness certificate.



Attempt 4 of run `4458418024444808b4bf3731b53b6cb7` reached overlay with every numbered HTTP exit 0, copied the generation-6 maintenance files, then exited 2. `export_oos.py` creates `product/oos` with `mkdir(exist_ok=False)`. The generation-5 `oos` directory from 14:24 was still there, so the call raised `FileExistsError`. Shared `product_flow.py` maps that to the generic refusal. A clean out directory exports the same `effective.json` successfully. The generation-5 tree was moved to `oos-g5-retained` and left intact. Attempt 5 then exported a new `product/oos` and wrote `product-receipt.json`.

`phase_floor` uses the run directory birth time only when this run's recovery verdict is verified, generation 6, and `source_write_attempts` is 0. A run without that verdict keeps the process-start floor. The unit test covers both.

A completed dialogue transcript is rechecked by launching `dialogue.py` with `resume_transcript`. The shell command keeps the `dialogue.py` path visible for 1.2 seconds so the onboard 0.5 second process sample records it. Attempt 5 recorded both programs. The recheck exited 0 with the same three query ids and an empty stderr.

Earlier failures stay on the run: `cancel-schema-invalid` (CLI exit 2, `SCHEMA_INVALID`, policy catalog unavailable, empty operations), `dialogue-gpt-failed`, `dialogue-grok-fenced`, `dialogue-grok-shape`, `dialogue-grok-timeout`, and whole attempts 0–4 exit 2. They were not rewritten as success.

The generation-6 catalog installed on the existing gateway is collection `b688c0e02b3809d7d959f478aa87a78e8bb10b9751d6455e25bf870bfa04fe60`, file SHA-256 `89fcdf8e3e02e0139257b693304033d071ee9a619be6367ad945540ae7f48a86`, pack digest `31b87ce91313a3378bc9d7355221da4e54708fc78d530d4139822dc4d0f11300`. Schema digest remains `9aac7b730f0a2df7fe6487fee9da336428ce780ce2bf6954c9595821c51ba8c6`. Maintenance `llm_calls` is 0. The generation-5 pack was not copied onto generation 6.

Linux rustc 1.94 failed to compile this crate because `TargetReport` now requires `capture_witness` and `CaptureMethod` now requires `delete_detection`. Both are optional and omitted from legacy JSON when unset. This crate now sets both to `None`. Shared `source-admission` was not edited. The same `cargo test --locked` then passed.

`~/.cache/oo-grok-connection-full-20261014/ROOT-OBSERVATIONS.json` still names run `a223a74bb93241f1b6ff5d771570eed2`. `INFRA-STATE.json` `product_pass` is false at `2026-10-14T05:44:28Z`. Those files are outside this write domain and were left as they are. ClickHouse, StarRocks, Oracle, and PostgreSQL engine-registry records were unchanged by the opensearch register. PostgreSQL's awaiting record was already present when this register ran.
