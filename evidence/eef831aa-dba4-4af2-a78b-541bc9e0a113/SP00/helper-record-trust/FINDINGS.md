# Findings — a113 helper record trust

Session `eef831aa-dba4-4af2-a78b-541bc9e0a113`. Slot B. Four checker fixes are in the working tree that this evidence describes. `audited`, `admission`, `fresh_current_scan`, and `release_signoff` stay false. This file does not approve the records, the scan, the licenses, or a release. Independent review of the commit that contains this evidence is still required. The coordinator owns checklists.

The cited frozen input was `0a9534d36799ca3bb76b77eba9237f4d415950c5` / tree `929fc357ea738b814367c704983698f932f0eb6d` / parent `d5aad63a5398e0a27b60f4634adf75008627d82c`. That commit is an ancestor of the parent used here. The parent at commit time is `d67795e397657087495a32317938ecdd7a6cb028` / tree `36225d138fc3f3bb1cbaea34d80b68eee7101034` / parent `5dce6c4b041134fcd017708edd28d0a9840416e0`. Checker blobs at that parent match product checker `abd074af5d59d413e12b8f615c50343e1655047f` for `check.rs`, `json.rs`, `lib.rs`, `main.rs`, `fail.rs`, `sha.rs`, `timeutil.rs`, `tests/closure.rs`, `Cargo.toml`, and `Cargo.lock`. Those last seven blobs are unchanged by this slice.

## F1 — adoption anchor

`src/trust.rs` pins commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0`, instant `2026-10-02T11:10:37Z`, and helper-lock sha256 `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. Host launch read author time and committer time `2026-10-02T19:10:37+08:00` and lock blob `2152785a3c9dc6713a62eb18052d7224bb5357eb` before the container started. The archive lock sha256 matched the pin. The checker compares inventory adoption and the lock file to those constants. Age is `completed_days` from `version.created_at` to that instant.

Spawn evidence in `linux/probe/trust-cases.tsv`:

- `aligned-adoption-age` exit 8, signal none, 10 ms, `code=invalid-time detail=adoption anchor`
- `aligned-age-recompute` exit 9, signal none, 11 ms, `code=age-floor detail=bitflags`

A later baseline needs a reviewed change to `trust.rs` and to evidence that rechecks the commit.

## F2 — MIT choice for this baseline

`bitflags`, `cfg-if`, and `libc` are pinned to MIT. `nix` and `cfg_aliases` stay a null choice. Inventory choice, record choice, and a nix native entry are compared with that pin. Apache-2.0 bodies and obligations stay required. This pin is the reviewed choice for these crates. Other Apache-2.0 text in the tree is outside it.

`aligned-choice-apache` rewrote all three inventory `"choice": "MIT"` values, the one record choice on each OR crate, and the three nix native MIT choices, then spawned the checker: exit 4, signal none, 9 ms, `code=wrong-dual-choice detail=bitflags`.

## F3 — retained a101 audit

Canonical `scan-audit.out` sha256 `f64e43716f72c173879cb30130f73df0f1c802ac32960eb6c3955c15c2209f81` was rechecked inside the container. The pin phase rejects a `status` field on that file. The checker does not write one. Required database identity is RustSec `117edb3bed98e9be112f277b7615eea3252e7c43`, time `2026-10-02T10:58:33+02:00`, advisory-count 1280, dependency-count 6, with settings, warnings, and an empty vulnerability list. A non-blank string status is `fake-claim` exit 19. A blank or non-string status, or a missing vulnerabilities object, is `malformed-record` exit 21.

Spawn rows, all signal none:

- `audit-status-unverified-aligned` exit 19, `detail=audit status`, digest rewritten to match the forged file
- `audit-status-number-aligned` exit 21
- `audit-status-blank-aligned` exit 21
- `audit-missing-vulnerabilities-aligned` exit 21
- `aligned-audit-commit` exit 19, `detail=audit database identity`, with inventory, records, and advisory-rev aligned to a forged commit

`canonical-bundle` exit 0, 40 ms, stdout exactly `helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false`.

cargo-audit and cargo-deny were not rerun. The retained scan bytes were not edited.

## F4 — JSON depth 64

Container depth only. `parse` starts at 0. A scalar root stays 0. An object or array root is depth 1. Each nested container adds one. The next depth is checked before the container function runs. Depth above 64 is `bound` exit 22, detail `json depth exceeds 64`. The existing 10_000-element and 4_000-key caps remain.

The record root is one container. Inserting 63 containers is total depth 64 and stays on the OK line. Inserting 64 is total depth 65. Spawn rows, all signal none, all under 40 ms:

- object, array, and mixed at inserted 63: exit 0
- object, array, and mixed at inserted 64: exit 22
- object, array, and mixed at inserted 20000: exit 22
- bad escape exit 21, invalid UTF-8 exit 21, duplicate key exit 14

a109 `depth-64` inserted 64 containers under the root. Under this rule that value is depth 65. The a109 observe row is the old parser's behavior. It is not the new acceptance boundary.

## Retained a109 RED

Source blobs of the unfixed checker at the parent match `abd074af`. The unfixed debug binary recorded by a109 is sha256 `c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`. That binary was not rebuilt. Rows from a109 `linux/probe/cases.tsv`:

| case | class | exit | ms | signal column |
| --- | --- | --- | --- | --- |
| paired-adopt-not-lock-commit | miss / fail | 0 | 43 | no |
| paired-choice-apache | miss / fail | 0 | 46 | no |
| depth-64 | observe | 0 | 48 | no |
| depth-20000 | observe | 134 | 25 | no |
| audit-status-unverified-aligned | miss / fail | 0 | 41 | no |

The first, second, and fifth printed the OK line. `depth-20000` stderr records a stack overflow abort. Oracle summary in the a109 report: `hits=38 misses=3 observes=5 cases=46`. This slice did not rerun that 46-case oracle.

## Attempts

| attempt | container | cargo test | clippy | smoke |
| --- | --- | --- | --- | --- |
| 1 | `6bc93623cb71` | 101 compile, `E0618` `Scratch` shadowed the function in `tests/trust.rs` | lib+bin 0 | 101 |
| 2 | `808b4329e41d` | 101. Unit 7 passed, closure 25 passed, trust 10 passed, 1 failed before spawn: inventory has three `"choice": "MIT"` and the test required one | `--all-targets` 101, `timeutil.rs:112` `clippy::assertions_on_constants` | 101 |
| 3 | `a5f2106f9b56` | 0. Unit 7, closure 25, trust 11 | lib+bin 0; `--test trust --test closure` 101 `needless_borrow` on `&ran` | 101 |
| green | `7e674b394cfa` | 0. Same counts | lib+bin 0; `--test trust --test closure` 0 | 0 |

Raw stdout, stderr, and exit files stay under `linux/attempt-1`, `linux/attempt-2`, and `linux/attempt-3`. `timeutil.rs` is outside the allow-list and was not edited. `cargo clippy --all-targets` is still red on that pre-existing assert. The clippy commands that exited 0 on the green run are the lib+bin command and `cargo clippy --test trust --test closure -D warnings`.

## Unsigned

Unknown record keys, hard links, manifest digest binding, pre-stat time-of-check/time-of-use, hash-subprocess timeout, and the hash child's environment were not exercised as acceptance tests. Depth behavior is proved for the object, array, and mixed cases at 64, 65, and 20000, and for the malformed escape, invalid UTF-8, and duplicate-key spawns above. That is the bound this run showed. It is not a general hostile-input claim. The checker is not a scanner, a legal owner, or a release signoff.
