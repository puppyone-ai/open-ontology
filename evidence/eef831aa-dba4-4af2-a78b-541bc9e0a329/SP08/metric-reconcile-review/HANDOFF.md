# Handoff

Owned path, and the only path this review wrote:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a329/SP08/metric-reconcile-review/**`

Product, root `Cargo.toml`, root `Cargo.lock`, `tools/metric-reconcile`, specs, and ledgers were not modified.

Verdict: **APPROVE_LIMITED** for the child calculation readiness at fixed commit `15558917e2c5be12b74dd1d64fb542b7d05a03af`.

`metric_certified: true` on `linux/review-3/live/pass.json` and `scale.json` means G5a and G5b passed for this closed-anchor comparison. `owner_authenticated`, `publication`, and `full_gates` are false. Omitted gates are `not_run`. docs/06 §3.7 owner certification and §6 publication are not approved. The parent SP08 goal is not accepted. No release is claimed. This review added no unit or rework count.

## Where the proof is

Accepting raw proof: `linux/review-3`. `docker-run.exit` 0, `review.exit` 0, `live.exit` 0. Reports and `checks.txt` are under `linux/review-3/live/`.

Earlier attempts stay in place:

- `linux/review`: archive missed `examples/schema/metrics-finance.yaml`. Test and clippy exit 101. Live skipped.
- `linux/review-2`: product smoke, tests, and clippy exit 0. Review-oracle compile exit 101. Live skipped.

Source blob of the crate at the fixed commit: `source/blobs/tools/metric-reconcile/`. `linux/review-3/source-diff.txt` is empty. Child lock sha256 `59599e433b3c025a0ec739989c0f9df74eebcc2f6cb9f9a96892523a10048948`. Root lock sha256 `16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a`.

Author packet, read and not rewritten: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a316/SP08/metric-reconcile`. The author's live fixture was not invoked.

## Resources

Cleanup has already run. `cleanup/cleanup.exit` is 0. Removed: containers `oo-a329-review`, `oo-a329-review-2`, `oo-a329-review-3`, `oo-a329-pg`; volumes `oo-a329-eef831aa-cargo`, `oo-a329-eef831aa-target`, `oo-a329-eef831aa-pgdata`; network `oo-a329-eef831aa-net`; directory `/tmp/oo-a329-eef831aa-dba4-4af2-a78b-541bc9e0a329`. No Docker prune. No other owner's containers were stopped.

Do not expect `oo-a329-pg` to answer. The saved JSON is the proof.

## Recipes recorded

`linux/review-3/origin-hashes.txt` lists a204 overlay-3 launch, a200 up / pg-wrap / extract, a199 preoverlay launch, and a316 preoverlay launch, extract, and pg-wrap, with the sha256 of each origin file. Review-3 changed the archive list so `examples/schema/metrics-finance.yaml` is present, and it names a329 resources. `rust-toolchain.toml` was set aside inside the private archive copy so the pinned 1.94.0 image remained the compiler.
