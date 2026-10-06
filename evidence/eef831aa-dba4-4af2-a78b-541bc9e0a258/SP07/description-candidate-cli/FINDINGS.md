# Findings

## What the CLI accepts

`description-candidate prepare_request` reads one `oo.builder.description-request-command/v1` document from stdin and calls `prepare_description_request`. `description-candidate complete_candidates` calls `complete_description_candidates`. The phase is a required argument. It is not inferred from `captured_response`. The public function names are rejected as unknown phases and are not printed back.

Exit 0 writes the function's string and does not add a newline. On the shop fixture that string is the library document: a description request, or an outcome whose candidates have `effect=review_only`. A budget-failure prepare is also that outcome document, with `candidates=[]` and the structural draft hashes.

## Closed exits

Exit 1 writes nothing to stdout. Stderr is one line, `description-candidate: <code> <message>`, using the public error fields. The error path is omitted. The proved line does not contain the shop owner, the hidden owner, the injection sentence, a duplicate-key sentinel, the oversize prefix, or the stale hash.

The 1048577-byte stdin is rejected by the CLI before the library call, with `oo.builder.description-request-command/v1/json.budget.bytes`. Non-UTF-8 stdin uses `json.syntax`. A duplicate key and a lone `{` use the library scanner codes. `prepare_request` with `captured_response` is `field.unknown`. `complete_candidates` without it is `field.missing`.

Hidden visibility and a physical table that the structure draft did not generate are both `selection.hidden`. A well-formed evidence hash that differs from the seal is `selection.stale`. Those exits leave stdout empty.

## Outcome documents that keep the draft

`incomplete` and `truncated` statuses, a complete body that names only one of the two selected datasets, and a complete body longer than `max_response_bytes` return the library outcome. `candidates` is empty. `structural_draft` keeps the sealed hashes. The planted label or response marker is absent from stdout. The compiled draft string held by the test is unchanged.

An injected label on a complete, in-budget response is stored as the label. Statements stay `publication=unavailable`, `llm_transport=not_used`, and `auto_accept=not_authorized`. The CLI does not interpret that label.

## Compiler notes

The pre-edit smoke and the proof both ran on rustc 1.94.0, Linux arm64, 6 GiB, 1 CPU. Clippy `-D warnings` on the new bin and `description_candidate_cli` exited 0. `Cargo.lock` was not edited.

No proof attempt failed. The first proof's tree diff matched the three owned paths.

## Outside this unit

No sample channel, no comment body channel, no column-level candidates, no temperature, cache, retry, provider transport, residency approval, or token cost. This file is the author record of the frozen CLI scope. It is not an independent acceptance review and it does not accept the parent S5 goal.
