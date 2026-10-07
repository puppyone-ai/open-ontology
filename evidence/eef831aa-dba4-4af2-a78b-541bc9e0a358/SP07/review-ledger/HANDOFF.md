# HANDOFF

Product commit: `f862683945a6e5a8803e3f9450d0f51cde5a98ad`.

Parent at that commit: `d22187ea0fce8e420323660841f2da3b447d5de8`.

Fixed base, ancestor: `a13b1cc80ed66c07199a41d47b31fd42530f2be9`.

Owned product path: `tools/review-ledger/**` only. Owned evidence path: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a358/SP07/review-ledger/**` only.

## Do not treat this as acceptance

SP07-W05 is not accepted. The parent item is not accepted. Counts stay unchanged until a fixed independent review. This session was the coder, the tester, and the writer of this packet. It was not that independent review.

## How to rebuild the proof

`proof/launch.sh` commands are `up`, `test`, `clippy`, and `restart`. `test` drops nothing by itself. It refuses a foreign container of the same name and refuses to start when ten `oo.kind=rust-builder` containers are already running. Images are local digests. `--pull never`.

The write-phase database already contains the history marker `da28bce2e8b4a697cd0589fa2ac2e47a9c1055e911ac32394ce4034ffd4833e6`. Running `launch.sh test` again on that database is not idempotent. A new write proof needs an empty `oo_a358_review`. `restart` only compares the marker.

Secrets, when the instance exists, are `/tmp/oo-a358-review-secrets` mode `0700`. The repository copy is the CA certificate hash in `proof/ca.sha256`.

## Next integration, still open

Management RBAC, HTTP, and Git curated apply are the next surfaces. They are outside this crate. See `FINDINGS.md`. An auditor `batch` write and a positive description-pack receipt are the two ledger behaviors this private PostgreSQL run did not close.

`commit.unknown` stays a non-retried error. Do not hide a lost commit reply.
