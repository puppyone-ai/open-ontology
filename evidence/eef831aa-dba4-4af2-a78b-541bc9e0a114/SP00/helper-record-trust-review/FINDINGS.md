# Findings — a114 helper record trust review

Verdict for F1–F4: APPROVE LIMITED. The five records stay `audited=false`. This review does not approve `audited=true`, admission, a fresh scan, or release signoff.

The spawn log is `linux/probe/cases.tsv`. The checker binary is `2cb04ba6701778408a5c09839af053c89d50614510546dff7a9e4ae1ec6adad7`. Expected rows were fixed in `expected-outcomes.tsv` before the container started, and that file was unchanged afterward. Every listed row has `signal=none` and `verdict=match`.

## F1 — adoption is the helper-lock instant

`paired-adoption-age` exit 8 in 21 ms. Stderr: `code=invalid-time detail=adoption anchor`. Stdout empty.

The probe rewrote `2026-10-02T11:10:37Z` once in the inventory and once in each of the five records, and rewrote the five stored ages to the completed-day values 14, 8, 70, 64, and 136. Those forged ages were computed from `version.created_at` to `2026-09-24T17:36:11.278420Z`. Each one is still above 168 hours. The checker rejected the instant.

`forged-age-real-adoption` kept the real adoption instant and decremented the five agreeing ages by one (22 to 21, 15 to 14, 77 to 76, 72 to 71, 144 to 143). Exit 9 in 21 ms. Stderr: `code=age-floor detail=bitflags`.

Host `git log` shows commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` at `2026-10-02T19:10:37+08:00` for both author and committer, which is `2026-10-02T11:10:37Z`. The archive lock sha256 is `14a0a0c66d604bae81f59b770e66a2d7cf00b38e427c3afbc8344bb4864189c1`. The probe recomputed the on-disk ages 22, 15, 77, 72, and 144 from `version.created_at` and matched the inventory. The age file records `floor_seconds=604800`.

## F2 — MIT is the choice for this baseline

`paired-apache-choice` exit 4 in 21 ms. Stderr: `code=wrong-dual-choice detail=bitflags`.

The case rewrote three inventory `"choice": "MIT"` values, the one `"dual_license_choice": "MIT"` on each of bitflags, cfg-if, and libc, and the three nix native `"dual_license_choice": "MIT"` values, all to Apache-2.0. License bodies were left as they are. The observed failure is the inventory pin for bitflags. The record and native rewrites were in the same spawned inputs. nix and cfg_aliases inventory choices stayed null.

## F3 — retained a101 audit identity

The canonical audit object has no `status` key. Its RustSec commit, update time, advisory count, dependency count, and empty vulnerability list are the values in `linux/audit-status-key.txt`. The probe did not invent a status for the passing case. Forged copies updated the inventory digest to the new file hash before the spawn, so a passing hash check is not an explanation for these exits.

| case | exit | ms | stderr |
| --- | --- | --- | --- |
| audit-status-unverified-aligned | 19 | 22 | `code=fake-claim detail=audit status` |
| audit-status-blank-aligned | 21 | 21 | `code=malformed-record detail=audit status` |
| audit-status-number-aligned | 21 | 21 | `code=malformed-record detail=audit status` |
| audit-missing-vulnerabilities-aligned | 21 | 21 | `code=malformed-record detail=missing vulnerabilities` |
| paired-rustsec-identity | 19 | 21 | `code=fake-claim detail=audit database identity` |

The RustSec case replaced `117edb3bed98e9be112f277b7615eea3252e7c43` in the audit file (1), the advisory rev (4), the inventory (1), and each record (1), and realigned the audit and advisory-rev digests. cargo-audit and cargo-deny were not rerun.

`canonical-five` exit 0 in 42 ms. Stdout is exactly `helper-evidence-check: OK records=5 audited=false license_record_status=complete-for-review release_signoff=false`. Stderr empty.

## F4 — container depth 64

The probe counts `{` and `[` outside strings. A scalar root is depth 0. The record root is one container. Inserting 63 containers under that root measures 64. Inserting 64 measures 65. Inserting 19999 measures 20000. The measured depth is the `depth` column. It was required to equal the pre-registered depth before the checker was spawned.

| case | depth | exit | ms | result |
| --- | --- | --- | --- | --- |
| depth-object-64, depth-array-64, depth-mixed-64 | 64 | 0 | 42–43 | OK line, `audited=false` |
| depth-object-65, depth-array-65, depth-mixed-65 | 65 | 22 | 20–22 | `bound`, `json depth exceeds 64` |
| depth-object-20000, depth-array-20000, depth-mixed-20000 | 20000 | 22 | 20–23 | `bound`, `json depth exceeds 64` |
| scalar-root-depth-zero | 0 | 21 | 21 | `malformed-record`, `missing package` |
| malformed-escape |  | 21 | 22 | `bad escape` |
| invalid-utf8 |  | 21 | 20 | `utf-8` |
| duplicate-key |  | 14 | 21 | `duplicate json key` |

No row reports a signal. Depth 20000 did not abort. The scalar file is the single token `0`. Its stderr has no depth-bound text.

The author's `tests/trust.rs` case named depth 20000 inserts 20000 containers under the record root, which is document depth 20001 under this counter. That integration test is one of the 11 trust tests that passed, so that deeper document also returned a typed failure inside the author's harness. The rows above are this review's measured depths.

## Retained a109 RED

Unfixed sources at the parent match `abd074af` and the a109 content hashes. The a109 oracle and `cases.tsv` hashes match `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a109/SP00/helper-license-review/HASHES.sha256`. The unfixed binary recorded there is `c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`. It was not rebuilt. Rows from that `cases.tsv`:

| case | class | exit | ms | signal column |
| --- | --- | --- | --- | --- |
| paired-adopt-not-lock-commit | miss | 0 | 43 | no |
| paired-choice-apache | miss | 0 | 46 | no |
| depth-64 | observe | 0 | 48 | no |
| depth-20000 | observe | 134 | 25 | no |
| audit-status-unverified-aligned | miss | 0 | 41 | no |

The first, second, and fifth printed the OK line. `depth-20000` stderr in that file records a stack overflow abort. Oracle summary: `hits=38 misses=3 observes=5 cases=46`. This review did not rerun that matrix.

## Author attempts left as they were

a113 attempt 1 `cargo-test-offline.exit` is 101, compile error `E0618` in `tests/trust.rs`. Attempt 2 trust result is 10 passed and 1 failed, and `--all-targets` clippy is 101 on `timeutil.rs:112`. Attempt 3 cargo test is 0 and test clippy is 101 on `needless_borrow`. Those files were read and not rewritten. This run's test clippy is 0. Its all-target clippy is 101 on the same `timeutil.rs:112` assertion. `timeutil.rs` was not part of the fix and was not edited here.

## Unsigned

Unknown keys, hard links, manifest binding, pre-stat time-of-check/time-of-use, hash-subprocess timeout, and the hash child's environment were not acceptance cases. The depth evidence is the table above. The checker is not a scanner, a legal owner, or a release signoff.
