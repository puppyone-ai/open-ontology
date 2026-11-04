# Resumable build handoff (partial S4-A-02)

`oo build --config stack.yaml --agent-target target.json` now requests the full
chain rather than returning success for an unpublished draft. It records:

1. `pull_selfbuild`: existing bounded `oo-load` and draft stages.
2. `self_review`: explicit owner review and real Builder recompilation.
3. `publish`: **blocked** at `publish_reviewed_registry` until lane M supplies
   the authenticated lite publication entry.
4. `gateway`, `first_token`, `share_prompt`: pending, not executed.

This is not a full-chain delivery claim. The Gateway consumes `_oo` publication
and policy state, whereas `oo-load` currently publishes `_oo_load` local table
views. Copying a local Pack or fabricating release gates cannot bridge that gap.

Use `--draft-only` explicitly for the previous metadata/draft-only behavior.
The wizard's existing bounded bridge still stops at draft. The existing live
draft runners use this flag; their previous evidence is not a new S4 run.

## Review input

The bounded description generator does not yet produce the community review
input described in `REVIEW.md`. Until its adapter is integrated, full build
stops with `BUILD_REVIEW_INPUT_REQUIRED`, without repeating generation.
Supply the operator-pinned community command using `--review-input FILE`, or
place it at `.oo/review-input.json`. Its additional `source_draft_sha256` must
match the compact sorted-key JSON hash of `.oo/draft.json`; it is a local
candidate binding, **not** authenticated source provenance.

`--review-file FILE` supplies the explicit per-item decisions in non-TTY mode.
Without it, a terminal opens the review TUI. No `--yes` path auto-approves.
Rejecting an item blocks the candidate, rather than partially publishing it.

On success, `.oo/reviewed-registry.json` contains the reviewed command, decision
ledger, effective OOS and compiled role Pack. A recovered output is recompiled
and compared before handoff, even if a crash occurred before its checkpoint
was saved.

## Resume and evidence

`oo build --resume` with the same config and Agent DB target validates the
configuration fingerprint and every completed handoff artifact. Changes refuse
resume. Once draft completion is checkpointed, no preflight/load/model call
is repeated. The underlying draft runner retains its own earlier-stage
checkpoints for interruption before that handoff.

`.oo/chain.json` is private and atomically replaced; a lock serializes full
builds. `oo status --config stack.yaml --json` reports this chain, including
the actual blocker, instead of calling it a successful draft or healthy Gateway.
An existing full-chain checkpoint cannot be mutated through `--draft-only`.

`test_build_chain.py` runs real processes, real SIGKILL and the actual review
compiler, but substitutes a deterministic external draft driver. It checks
draft/review boundaries, identical Pack digest, no repeated generation,
late review-input recovery, and artifact-tampering refusal. It does **not**
test the three unimplemented post-publication boundaries or any live database.

Integration entry: `build.rs::publish_reviewed_registry`. It must call lane M's
authenticated release path, not return a synthetic success. Gateway validation,
durable first-user credential issuance and lane B's `oo share-prompt` must be
connected after that contract exists; exactly-once issuance also needs recovery
of an interrupted issuance rather than blindly issuing another credential.
