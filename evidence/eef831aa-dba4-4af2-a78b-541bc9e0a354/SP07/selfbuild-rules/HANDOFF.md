# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a354`. Module `tools/selfbuild-rules`. Contract `tools/selfbuild-rules/CONTRACT.md` (`oo.selfbuild-rules/v1`). Parent SP07 acceptance is still open. A reviewer should treat this as author delivery, not as acceptance.

## Run

The crate is a standalone package with an empty `[workspace]`. Rust 1.94, `--locked`, no root manifest change. Binary name `selfbuild-rules`.

```
selfbuild-rules produce --catalog CATALOG --profile PROFILE --cards CARDS --auto-accept rule_high_confidence
selfbuild-rules score --truth TRUTH --actual REVIEW
```

Exit 0 is a review document or a matching score. Exit 1 is a refusal or `score.mismatch`. Exit 2 is usage. Stderr is `selfbuild-rules:` plus a static code. A produce refusal writes no stdout. A score mismatch still writes the score JSON.

The sealed shop inputs used by the proof are only these three files, read-only:

- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2/catalog.json`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2/profile.json`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a328/SP07/pg-builder/live/pass2/cards.json`

`cargo test` skips `sealed_shop_cards_match_the_hand_rows` unless `SEALED_CATALOG`, `SEALED_PROFILE`, and `SEALED_CARDS` are set. The exit-0 proof set them and the test passed.

## Reviewer checks

- Hand oracle is `tools/selfbuild-rules/tests/fixtures/d-truth.json`. Do not replace it with producer output.
- Expected demo shape: 26 columns, 7 relations, 4 enums, 1 dictionary, 1 conflict, 20 experts, leak 0.
- Shop shape: 14 columns, 2 declared foreign keys, 5 experts, leak 0, and the four forbidden substrings absent.
- `linux/proof/docker-run.exit` is 0. Attempts 1–5 stay at exit 101. `linux/smoke/docker-run.exit` stays 1.
- `SHA256SUMS` lists the owned files, including `SOURCE-INVENTORY.txt`. `SHA256SUMS.sha256` is the digest of `SHA256SUMS` and is not a line inside it.
- Lock subset log: `registry_triples=11 missing=0 mismatch=0`.

## Resources left in place

Network `oo-a354-eef831aa-net`. Volumes `oo-a354-eef831aa-cargo` and `oo-a354-eef831aa-target`. They are this UUID's private resources. No container named `oo-a354-rules-proof` remains. No PostgreSQL was started. No prune, push, or release was run.

## Out of scope for the next edit

Do not edit `tools/pg-builder`, `crates/oo-core`, the root manifests, or the frontend from this unit. Do not accept SP07 from this packet alone.
