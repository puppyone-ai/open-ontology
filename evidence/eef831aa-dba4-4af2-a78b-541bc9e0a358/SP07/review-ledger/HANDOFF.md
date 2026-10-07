# HANDOFF

Amendment product commit: `6b3a92c2e06a2e91a74393b739ec8f1c565ae17b`.

Parent of that commit: `a08b3576969cf06c097a4769bce8f9a4758a81a2`.

Earlier product commit, still in history: `f862683945a6e5a8803e3f9450d0f51cde5a98ad`.

Fixed base, ancestor: `a13b1cc80ed66c07199a41d47b31fd42530f2be9`.

Owned product path: `tools/review-ledger/**` only. Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a358/SP07/review-ledger/**` only.

## Do not treat this as acceptance

E52 is not accepted. SP07-W05 is not accepted. The parent item is not accepted. Counts stay unchanged until a fixed independent review. This session was the coder, the tester, and the writer of this packet. It was not that independent review.

## How to rebuild the proof

`proof/launch.sh` commands are `up`, `test`, `clippy`, and `restart`. Logs and the history marker go to `proof-fix/`. `test` requires `Cargo.lock` and does not run `cargo generate-lockfile` or Clippy. Clippy is the separate `clippy` command. `test` refuses a foreign container of the same name and refuses to start when ten `oo.kind=rust-builder` containers are already running. Images are local digests. `--pull never`.

The amendment write-phase database contained history marker `c7e08e02cde66ab86b75394488f0d263ec9475cecc75884599de3703db8264ce` in `proof-fix/history.sha256`. The earlier marker `da28bce2e8b4a697cd0589fa2ac2e47a9c1055e911ac32394ce4034ffd4833e6` remains in `proof/history.sha256` for commit `f862683945a6e5a8803e3f9450d0f51cde5a98ad`. Running `launch.sh test` again on a populated database is not idempotent. A new write proof needs an empty `oo_a358_review`. `restart` only compares the marker.

Secrets, when the instance exists, are `/tmp/oo-a358-review-secrets` mode `0700`. The repository copy for this amendment is the CA certificate hash in `proof-fix/ca.sha256`.

## Next integration, still open

Management RBAC, HTTP, and Git curated apply are the next surfaces. They are outside this crate. See `FINDINGS.md`.

`commit.unknown` stays a non-retried ambiguous error. Do not hide a lost commit reply and do not retry it blindly.
