# Findings

Goal remains open. Two fixture I/U/D rounds on the existing binding advanced generation 6 to 12. Every capture had `reader_mutations` 0. After each delete the live document set returned to the five stable keys. The schedule is enabled at 1000 ms. The owned observer calls `native-step` only when the C5 probe reports a row diff. Quiet ticks have left generation 12 in place. Daemon pid 66510 exited on a Docker socket error after 203 quiet ticks. Replacement pid 56377 is the running observer. The receipt snapshot is ticks 255, fires 0.

The public gateway now serves generation 12. Maintenance artifact `aa6ac5bb99cbff9a11dd02a6c1a4ce0d4ce33b85e274ba9a0fa58d0e84736cae`, `llm_calls` 0, publication `not_published`, role-pack digest `417c06543511aef09334f937153e1a6747900af2d601dcb42d023c21e6c3eec1`. Collection `f73d050aaa2c372aa78b05fc891b6e3058661d6538dd0eb9386f1c4a0f86292d`. Canonical catalog SHA-256 `22ef85b7e799d009753a590be4ed504a841cc64514b15794032055dff0b35433`. `oo_r_ledger` has SELECT on the 14 business columns of `pt_673a7926ef6d97ac56fbe19b6fcf10ce4357a5e17f30d95ea28a926d7d20`, no table privilege, and no future default SELECT. Installer receipt `78ecad1e615ec59c4cb563892f6a388ec2c341c702981fe162a018f19b0a0b19`, ELF `78bfdc62bd09328837c694adcf091404b70b0dfd9d4a2928540315f3bf10b60a`, source `afaa67da5ed842884c524a0e7d5e5406ac4ee603`. This source did not GRANT.

Public 14-column gold is query `q_J2xS9Y546NOQVNqWIg4qkpLzZv`, 5 rows, 70 cells, 0 differences, generation 12 bound. Verdict SHA-256 `b9a73e3f02efb7fc2e3385e0f3684fac3b599fa20a6a6f36667417ac02723f9a`. `STALE_DATA` remains because the watermark is `awaiting_publish`. It is not a freshness certificate. Queries `q_7eZbKDsfbIsqIvIeXfwGZQ3BxC` and `q_pkHbG7YWDiO5VC8xBVNl9XX2Z7` are the pre-rebind `SCHEMA_INVALID` record.

The current-facts dialogue is verified. Verdict SHA-256 `a06fcc97e4c5a383659fffc484803610fe6b35ad3bde48af5b597e916c80011c`. Query ids `q_WmJg2c15AP4hrifaxSGi4jgz2A`, `q_cMhrZwR4R4t72sXrUbmA51KBtp`, and `q_tvPHkytegb3Rb36bU1vybRLDIy` are in the WAL. Live cancel `q_LL4oGbozXAKiNlMLQcXUsTKqnv` returned sqlstate 57014 with rollback. Formal sign-off remains 0/10. Linux rustc 1.94.0 passed the same 6 library tests again on `/target/oo-os-preview-msrv`.

The global `engines.json` digest moved from the resume identity `51308e7a…` through `358c6c75…` to `29ddf63d82d24b623683b980a722fa7cef8535769c8cfb920be4ea6274eaacd2`. The opensearch row stayed `whole: true` with the same recipe and proof. That resume drift stays with `d28ca4bd`. `onboard.py` was not edited. The old run was not resumed and its journal pins were not rewritten.

Declared capture is C5, consistency B, `delete_detection` reconcile, PIT `snapshot_token` false. The ontology file does not claim a change stream. `STALE_DATA` and `awaiting_publish` stay as stored. They are not a formal freshness certificate.



Attempt 4 of run `4458418024444808b4bf3731b53b6cb7` reached overlay with every numbered HTTP exit 0, copied the generation-6 maintenance files, then exited 2. `export_oos.py` creates `product/oos` with `mkdir(exist_ok=False)`. The generation-5 `oos` directory from 14:24 was still there, so the call raised `FileExistsError`. Shared `product_flow.py` maps that to the generic refusal. A clean out directory exports the same `effective.json` successfully. The generation-5 tree was moved to `oos-g5-retained` and left intact. Attempt 5 then exported a new `product/oos` and wrote `product-receipt.json`.

`phase_floor` uses the run directory birth time only when this run's recovery verdict is verified, generation 6, and `source_write_attempts` is 0. A run without that verdict keeps the process-start floor. The unit test covers both.

A completed dialogue transcript is rechecked by launching `dialogue.py` with `resume_transcript`. The shell command keeps the `dialogue.py` path visible for 1.2 seconds so the onboard 0.5 second process sample records it. Attempt 5 recorded both programs. The recheck exited 0 with the same three query ids and an empty stderr.

Earlier failures stay on the run: `cancel-schema-invalid` (CLI exit 2, `SCHEMA_INVALID`, policy catalog unavailable, empty operations), `dialogue-gpt-failed`, `dialogue-grok-fenced`, `dialogue-grok-shape`, `dialogue-grok-timeout`, and whole attempts 0–4 exit 2. They were not rewritten as success.

The generation-6 catalog installed on the existing gateway is collection `b688c0e02b3809d7d959f478aa87a78e8bb10b9751d6455e25bf870bfa04fe60`, file SHA-256 `89fcdf8e3e02e0139257b693304033d071ee9a619be6367ad945540ae7f48a86`, pack digest `31b87ce91313a3378bc9d7355221da4e54708fc78d530d4139822dc4d0f11300`. Schema digest remains `9aac7b730f0a2df7fe6487fee9da336428ce780ce2bf6954c9595821c51ba8c6`. Maintenance `llm_calls` is 0. The generation-5 pack was not copied onto generation 6.

Linux rustc 1.94 failed to compile this crate because `TargetReport` now requires `capture_witness` and `CaptureMethod` now requires `delete_detection`. Both are optional and omitted from legacy JSON when unset. This crate now sets both to `None`. Shared `source-admission` was not edited. The same `cargo test --locked` then passed.

`~/.cache/oo-grok-connection-full-20261014/ROOT-OBSERVATIONS.json` still names run `a223a74bb93241f1b6ff5d771570eed2`. `INFRA-STATE.json` `product_pass` is false at `2026-10-14T05:44:28Z`. Those files are outside this write domain and were left as they are. ClickHouse, StarRocks, Oracle, and PostgreSQL engine-registry records were unchanged by the opensearch register. PostgreSQL's awaiting record was already present when this register ran.
