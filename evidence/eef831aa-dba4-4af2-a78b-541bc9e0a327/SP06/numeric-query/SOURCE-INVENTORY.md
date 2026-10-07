# Source inventory

Owned by a327:

| Path | Role |
| --- | --- |
| `execution/decisions/numeric-query-contract-a327.md` | Frozen contract `3517cd13`, allowlist addendum `c4d9316d`. |
| `crates/oo-gateway/src/reviewed_policy.rs` | `numeric` base type. Compatible with quantity only. `c671a3ca`. |
| `crates/oo-gateway/src/query_executor.rs` | OID 1700 exact decimal decode and `Cell::Decimal`. `c671a3ca`. |
| `crates/oo-gateway/src/bin/query-pg.rs` | Decimal JSON cell, `row_filter` null, and the `a327_live` harness. `976cb453`. |
| `crates/oo-gateway/tests/a327_numeric.rs` | Hand literals. No wire decoder and no live database. |
| `crates/oo-gateway/src/policy_catalog.rs` | `SUPPORTED_TYPES` includes bounded numeric. `e9cba7df`. |
| `crates/oo-gateway/src/policy.rs` | `ensure_grantable_type` uses `grantable_base_type`. `e9cba7df`. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/**` | Smoke, post-edit, failed attempts, private PostgreSQL, live-3 cells. |

Not owned. Reported seam:

| Path | Why it matters |
| --- | --- |
| `crates/oo-gateway/src/mapping_reader_access.rs` | `ensure_supported_type` matches the type name only. No `atttypmod` bound. Diff against `e6ebf93` is empty. |

Proof overlay hashes from `linux/live-3/overlay-sha256.txt`. The same digests match the worktree files at `976cb453`, except `Cargo.lock`: the proof hashed the archive lock, and the workspace lock is `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474`.

```
3cf525604513d3a64be2e5689e9011986a079a342f1bac6c863438ae5d34a242  crates/oo-gateway/src/reviewed_policy.rs
46f7a8d79af146f5199896ad0ac820fe5415e39fd9530a6a887e7dbbc8286a6d  crates/oo-gateway/src/query_executor.rs
adfc5af184eac880eb35377222fb7e08416d457b57d7cd94767e71947574d169  crates/oo-gateway/src/bin/query-pg.rs
ac298ba546c7cea9595982dcb0341f2fe60b3dc9b3115ca89f070aad77251a2b  crates/oo-gateway/tests/a327_numeric.rs
63b28a2cd5da4914b124d7eac42d7d8b815033d96208118d3fb4c94b242f4325  crates/oo-gateway/src/policy.rs
1d914418d587a550984858e4f11874a80a0d0f5ea8a1b9861f9d789a4a60ca67  crates/oo-gateway/src/policy_catalog.rs
16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a  Cargo.lock
```

`linux/live/overlay-sha256.txt` is the earlier four-file overlay. Its `query-pg.rs` digest is `aed7a9e7b4565d771b99c234175f1866b1fae3b40103e791d1f09e045dd0a803`. That run stopped at install exit 7.

`SHA256SUMS` in this directory is the Rust `sha256 dir` inventory of the evidence tree. `tools/sha256.rs` produces it. The inventory container is `oo-a327-hash`, label `oo.kind=hash`, network none, 512 MiB, 1 CPU, the pinned Rust image. It is not a rust builder.
