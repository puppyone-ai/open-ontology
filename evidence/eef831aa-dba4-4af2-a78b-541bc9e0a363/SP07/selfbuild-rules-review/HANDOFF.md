# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a363`. Frozen product commit `e5427c1a18e2ddbb480791d214c76888eb7ca998`. Contract `tools/selfbuild-rules/CONTRACT.md` (`oo.selfbuild-rules/v1`).

Decision: APPROVE_LIMITED

This packet covers the frozen review-only producer. SP07 acceptance stays open. No new product unit was added. Parent and goal acceptance stay open. A later editor should leave `tools/selfbuild-rules`, the root manifests, the sealed pass2 files, and the a354 author packet unchanged unless a new task says otherwise.

## Evidence map

- `linux/smoke/` is the image baseline. `docker-run.exit` 1, Clippy absent. Do not rewrite it and do not treat it as a pass.
- `linux/proof/` holds the passed product campaigns and the failed first oracle compile. `proof.exit` stays 1. Do not rerun the passed phases and do not overwrite `console.log`.
- `linux/oracle-2/` is the oracle-only retry. `docker-run.exit` 0, `oracle.log` is `oracle_ok`, cli bytes match the preserved shop review.
- `oracle/main.rs` is the hand oracle. Expected rows are literals in that file.
- `source-product.sha256` lists the 14 frozen product files.
- `tools/lock_subset.rs` is the a354 checker, byte-identical.

## Resources left in place

Network `oo-a363-eef831aa-net`. Volumes `oo-a363-eef831aa-cargo` and `oo-a363-eef831aa-target`. They are this UUID's private Rust cache and target. No container named `oo-a363-rules-smoke`, `oo-a363-rules-proof`, or `oo-a363-rules-oracle2` remains. No prune, push, or release was run. This review did not start or remove a PostgreSQL container.

## Commands the proof already ran

```
cargo test --locked --offline --manifest-path /work/Cargo.toml
cargo clippy --locked --offline --all-targets --manifest-path /work/Cargo.toml -- -D warnings
selfbuild-rules produce --catalog /sealed/catalog.json --profile /sealed/profile.json --cards /sealed/cards.json --auto-accept rule_high_confidence
```

The oracle retry compiled `oracle/main.rs` with the existing debug rlibs and ran `/target/a363-oracle2`. It did not call `cargo test`, `cargo clippy`, `cargo fetch`, or the sealed CLI again.

## Reviewer notes for the next reader

- Gold sha256 `2f3adbeeb3037b547d33eb611d043e9ee11b52e32b4983fd3b9148575f928f66` must stay the author hand truth. Do not replace it with producer output.
- Author fixture `n_distinct` 40 keeps the gold `many_to_one` case. The low-cardinality declared-FK enum case is `linux/oracle-2/oracle-low.json`, where `n_distinct` is 5, the semantic type stays `foreign_key`, and two undetermined enum cards are present.
- Shop shape: 14 columns, 2 declared foreign keys, 0 enums, 5 experts, leak 0.
- Lock subset: `registry_triples=11 missing=0 mismatch=0`.
- `SHA256SUMS` lists the owned files, including `SOURCE-INVENTORY.txt`. `SHA256SUMS.sha256` is the digest of `SHA256SUMS` and is not a line inside it.

## Out of scope

Frontend sessions a351, a353, and a352. Full S2–S4 containment checks, join paths, model adjudication, and publication. Those are outside the frozen contract this unit implements.
