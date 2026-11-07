# Resumable build handoff (partial S4-A-02)

`oo build --config stack.yaml --agent-target target.json` now requests the full
chain rather than returning success for an unpublished draft. It records:

1. `pull_selfbuild`: existing bounded `oo-load` and draft stages.
2. `self_review`: explicit owner review and real Builder recompilation.
3. `publish`: **blocked** at `publish_reviewed_registry`: D15 activation exists,
   but the current authenticated publisher rejects the retained-cut contract.
4. `gateway`: fail-closed start/reload seam pending lane M's publication contract.
5. `first_token`: lane B's authenticated issue API with durable retry material,
   private credential and receipt.
6. `share_prompt`: lane B `oo share-prompt`, separate short-lived enrollment code
   in a private JSON receipt, never stdout of the outer build.

Stages 5 and 6 are implemented but unreachable while the publication and Gateway
seams fail closed. This does not claim live token or Gateway acceptance.

This is not a full-chain delivery claim. `oo-load publish` now prepares retained
cuts and D15 activation, but requires reviewed open fences. The existing
`release-coordinator` publisher accepts the legacy library fixture and rejects
the retained consistency, receipt protocol and non-UUID generation independently
(probe `a3-s4-publisher-contract`). Direct fixture SQL in the Gateway-cycle harness
is not authenticated publisher acceptance. Copying a local Pack or fabricating
release gates cannot bridge that gap.

Use `--draft-only` explicitly for the previous metadata/draft-only behavior.
The wizard's existing bounded bridge still stops at draft. The existing live
draft runners use this flag; their previous evidence is not a new S4 run.

## Review input

The sealed catalog/cards adapter invokes lane G's community/three-pass runner,
not the legacy single-pass description generator. It writes the community review
input described in `REVIEW.md` to `.oo/review-input.json`. Classification proposals
default to `restricted` (not confirmed), and every column still requires an
explicit decision. The runner prints its structural preflight and retains it in
`.oo/community/preflight.json`; the wrapper reserves a durable six-call limit
including failed/interrupted calls. More than four planned batches is refused.

The optional `--review-input FILE` must match an existing generated input.
Its additional `source_draft_sha256` must
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

Integration entries: `build.rs::publish_reviewed_registry` and
`build_tail.rs::gateway`. Neither returns synthetic success. Lane M must supply
an authenticated, replay-safe publication call and an owned Gateway start/reload
operation with a real health/publication probe. The latter must produce a private
receipt containing `protocol: oo.build-publication/v1`, `published: true`,
`gateway_verified: true`, `gateway_url`, `ca_file`, `client_pem` and the authorized
lane-B `first_user_grant`. These fields are a proposed adapter contract, not a
claim that lane M has accepted or implemented it.

First-user issuance now reserves CSPRNG token material, the idempotency key and
the publication/grant binding in atomic private `first-token-request.json`
before dispatch. It calls lane B's existing mTLS admin route with optional
`retry` material. The server stores only hashes and serializes retries with the
project lock. It refuses changed bindings and never revives expired, revoked or
rotated credentials. Install the additive token-store extension with the
existing `issue-token install` before use; accepted v1/v2 migrations are unchanged.

`cargo test --locked -p oo-cli` tests real SIGKILL at request, committed-response
and receipt boundaries with a synthetic remote. For a real PostgreSQL 17.11 +
mTLS Gateway remote, run
`python3 tools/gateway-token-store/test_issue_retry.py --run-id UNIQUE --cli`.
Run `a3-s4-token-mtls-kill-2` kills the first-token client after the real remote
commit and observes exactly one credential after recovery. This tests lifecycle
only, not a published Pack, source ingestion, or a Gateway data query.

Legacy first-token intents without reserved material still require audit
reconciliation; guessing their outcome could duplicate a credential. The
share-prompt enrollment stage also still uses a fail-closed intent: its lost
response is not replay-safe. Publication, Gateway startup, enrollment recovery,
printed-prompt delivery and the three-engine full build remain incomplete.
