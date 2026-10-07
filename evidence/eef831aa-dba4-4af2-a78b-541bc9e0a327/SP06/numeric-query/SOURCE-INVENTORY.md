# Source inventory

Owned by a327:

| Path | Role |
| --- | --- |
| `execution/decisions/numeric-query-contract-a327.md` | Frozen contract, commit `3517cd131719cfdd572ba2b85bb7013e6c90588e`. |
| `crates/oo-gateway/src/reviewed_policy.rs` | `numeric` base type. Compatible with quantity only. Commit `c671a3ca`. |
| `crates/oo-gateway/src/query_executor.rs` | OID 1700 exact decimal decode and `Cell::Decimal`. Commit `c671a3ca`. |
| `crates/oo-gateway/src/bin/query-pg.rs` | Decimal JSON cell, plus the `a327_live` proof harness. |
| `crates/oo-gateway/tests/a327_numeric.rs` | Hand literals. No wire decoder and no live database. |
| `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a327/SP06/numeric-query/**` | Smoke, post-edit, failed attempts, private PostgreSQL scripts, live boundary. |

Not owned, and the live boundary:

| Path | Why it matters |
| --- | --- |
| `crates/oo-gateway/src/policy_catalog.rs` | `SUPPORTED_TYPES` has no `numeric`. Catalog load returns unsupported type. |
| `crates/oo-gateway/src/policy.rs` | `ensure_grantable_type` uses that list. Install exits 7. |

Proof overlay hashes from the last live builder (`linux/live/overlay-sha256.txt`):

```
3cf525604513d3a64be2e5689e9011986a079a342f1bac6c863438ae5d34a242  reviewed_policy.rs
46f7a8d79af146f5199896ad0ac820fe5415e39fd9530a6a887e7dbbc8286a6d  query_executor.rs
aed7a9e7b4565d771b99c234175f1866b1fae3b40103e791d1f09e045dd0a803  query-pg.rs
ac298ba546c7cea9595982dcb0341f2fe60b3dc9b3115ca89f070aad77251a2b  a327_numeric.rs
16e6afda29ae4bae251d569f3c4ad16459527f0d0323598d8e4e0971818d458a  Cargo.lock
```

`SHA256SUMS` in this directory is the Rust `sha256 dir` inventory of the evidence tree. It is produced by `tools/sha256.rs`.
