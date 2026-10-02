# Findings — a109 helper license review

Scoped verdict: request-changes. The five records stay `audited=false`. This review does not approve `audited=true`.

The real archive check exits 0 and prints `audited=false`. That is the checker accepting the unaudited bundle. The independent probe also found three cases where two forged claim files agree and the checker still exits 0, plus one hostile record that aborts the process.

Raw rows are in `linux/probe/cases.tsv`. The oracle called the archive binary `helper-evidence-check` (`c2a7b9bae81198010c0cfa6b682a71087a2df3541c14c16fd7df7f17d4ca8694`). Each row keeps the flattened stdout and stderr from that process.

## F1 — adoption is the inventory string

`paired-adopt-not-lock-commit` exit 0 in 43 ms. Class miss.

The oracle rewrote `adoption_date` in the inventory and in all five records from `2026-10-02T11:10:37Z` to `2026-09-24T17:36:11.278420Z`, and rewrote `age_days_at_adoption` to the completed-day values 14, 8, 70, 64, and 136. The checker printed the OK line.

`check_time` requires the record adoption string to equal the inventory adoption string, requires that instant to be between publication and `--as-of`, and requires the stored day count to equal the completed days and to be at least 168 hours. It does not read the helper lock commit. The lock commit `f1a6005971e3dc3075094c87ed670fc9d25e9ae0` has author and committer time `2026-10-02T19:10:37+08:00`, which is the recorded adoption instant. A later pair of claim files can name a different instant and still pass the floor.

The on-disk records still say `2026-10-02T11:10:37Z`. Ages on disk are nix 144, libc 72, cfg_aliases 77, bitflags 22, cfg-if 15. The oracle's copy of the civil-day formula matched those five vectors before any case ran.

## F2 — selected license is the agreeing pair

`paired-choice-apache` exit 0 in 46 ms. Class miss.

The oracle changed the first inventory `"choice"` from MIT to Apache-2.0, the bitflags record `dual_license_choice`, and the first nix `native_transitive` `dual_license_choice`. License files and obligation sentences were left as they are. The checker printed the OK line.

`check_choice` accepts an OR expression when the record choice equals the inventory choice. It does not require the choice to be MIT. `check_obligations` checks that each SPDX id in the expression has a non-placeholder duty. It does not require the duty text to change when the choice field changes. Record-only choice (inventory left as MIT) failed closed: exit 4, `wrong-dual-choice`, 21 ms.

The archive records for bitflags, cfg-if, and libc still say MIT, and both license bodies are present. nix and cfg_aliases are MIT-only with a null choice. The miss is that a coordinated edit of the claim files can record Apache-2.0 and the checker exits 0.

## F3 — audit `status` is unread

`audit-status-unverified-aligned` exit 0 in 41 ms. Class miss.

The oracle appended `"status":"unverified"` to the retained `scan-audit.out` object and updated the inventory digest to the new file hash. The checker printed the OK line.

`check_audit_file` reads `database.last-commit`, `database.last-updated`, `lockfile.dependency-count` (must be 6), and `vulnerabilities.found/count/list`. It does not read a status field. Unknown JSON keys are stored and ignored.

These neighboring cases failed closed:

- Missing audit file: exit 11, `scanner-output-missing`, 23 ms.
- One flipped audit byte with the old inventory digest: exit 3, `hash-mismatch`, 21 ms.
- Deny summary `"errors":1` with the inventory digest updated: exit 19, `fake-claim`, `deny advisories were not empty`, 21 ms.
- RustSec commit forged in the inventory and the records together: exit 19, `audit database identity`, 21 ms.

The retained scan is still the a101 file. `fresh_current_scan` is false on disk. This review did not rerun cargo-audit or cargo-deny. A file that still has zero vulnerabilities and also says `status=unverified` is accepted once its digest is what the inventory names.

## F4 — JSON depth has no bound

`depth-64` exit 0 in 48 ms. Class observe.

`depth-20000` exit 134 in 25 ms. Stderr: `thread 'main' (1452) has overflowed its stack fatal runtime error: stack overflow, aborting`.

`json::parse` recurses in `value` / `object` / `array`. Arrays stop at 10,000 elements and objects stop at 4,000 keys. There is no depth counter. A deep object in one record aborts the process. The card asked for a finite depth boundary. Depth 64 is accepted. Depth 20,000 is a stack abort, exit 134, rather than `malformed-record` or `bound`.

## Observations the probe did not score as required fails

These exited 0. They are limits of the checker. They are not a pass of F1–F3.

- `unknown-key`: a `reviewer_note` string inserted beside the real keys. Exit 0 in 42 ms. Duplicate keys failed: literal and `\u0061udited` both exit 14.
- `hardlink-same-bytes`: the license path and an outside path share an inode (`nlink=2`) and the bytes match the pinned digest. Exit 0 in 43 ms. Symlinks are rejected.
- `manifest-oneline`: a file whose only line is `license = "Apache-2.0"`. Exit 0 in 43 ms. `check_helper_manifest` keeps the license lines and compares them to that one string. The helper manifest is not digest-bound. The positive case used the archived `Cargo.toml`.
- `sha.rs` starts `/usr/bin/sha256sum`, or else `/usr/bin/shasum` with a fixed `-a` `256`, through `Command::new`. There is no shell. Arguments are the binary, the fixed prefix, and the path. The PATH probe wrote a `sha256sum` earlier on `PATH` that would create `/tmp/oo-a109-sha-marker`. The marker was absent and the check exited 0 in 43 ms. The child inherits the environment, and `sha256_file` sets no timeout. Fifo and symlink cases returned in under 30 ms, so this run did not observe a hang.

## What failed closed

| Case | Exit | Code | ms |
| --- | --- | --- | --- |
| positive real archive | 0 | OK, audited=false | 46 |
| paired checksum | 18 | checksum-mismatch, lock | 21 |
| paired version 9.9.9 | 18 | checksum-mismatch, lock | 26 |
| paired repo with user:pass URL | 15 | wrong-source, version json | 21 |
| paired publisher/crate created_at | 8 | invalid-time, age file publication | 23 |
| paired feature | 20 | graph-mismatch | 27 |
| record-only choice | 4 | wrong-dual-choice | 21 |
| obligation replaced with n/a | 17 | placeholder | 20 |
| paired RustSec commit | 19 | fake-claim, audit database identity | 21 |
| CLI `..`, source `..`, absolute, URL credential, `\u002e\u002e`, `\u0000` | 12 | path-traversal | 21–28 |
| symlink license | 12 | refusing symlink or non-file | 23 |
| fifo license | 2 | missing-license-body | 27 |
| fifo lock | 10 | helper-lock-changed, required file missing | 27 |
| duplicate key, unicode duplicate key | 14 | duplicate json key | 21 |
| invalid UTF-8 | 21 | utf-8 | 25 |
| NaN age | 21 | unsupported json value | 25 |
| null publication | 21 | expected string | 25 |
| oversize license | 22 | file too large | 23 |
| 12 extra record files | 22 | too many record files | 23 |
| missing Apache, hash-aligned damaged Apache | 6 | omitted-apache-body, libc | 45, 42 |
| missing NOTICE, hash-aligned damaged NOTICE | 7 | missing-third-party-notice, cfg_aliases | 21, 23 |
| audited, admission, fresh_current_scan, release_signoff true | 19 | fake-claim | 22–26 |
| as-of one second before adoption | 8 | invalid-time | 21 |

Fifo rejection uses `Path::is_file`, which is false for a fifo, before `read`. The license fifo therefore returns missing-license-body (exit 2). The lock fifo returns helper-lock-changed (exit 10). Neither open blocked.

## Upstream bytes

Official `https://static.crates.io/crates/{name}/{name}-{version}.crate` downloads matched the lock checksums. License bodies were byte-equal to the canonical provenance copies and to the a101 retained copies. Log: `upstream/crate-compare.txt`.

- bitflags 2.13.2 and cfg-if 1.0.5: `MIT OR Apache-2.0`. Apache file sha256 `a60eea817514531668d7e00765731449fe14d059d3249e0bc93b36de45f759f2`, 10847 bytes, ends after END OF TERMS with the appendix line `Copyright [yyyy] [name of copyright owner]`. Records say `apache_appendix_copyright` `standard-placeholder-not-a-named-owner`.
- libc 0.2.189: Apache sha256 `62c7a1e35f56406896d7aa7ca52d0cc0d272ac022b5d2796e7d6905db8a3636a`, 9723 bytes, last line `END OF TERMS AND CONDITIONS`. No placeholder appendix. Record appendix is null. `open_gates.platform_c_library` is `open`.
- cfg_aliases 0.2.2: published manifest has edition 2018 and no `rust-version`. Record MSRV is `not declared in the published manifest or the crates.io version object; edition 2018`. NOTICE sha256 `1e2b7ade3fb228130408b9990cae6a7618eb314c75aa0b164bfe485d9d9756ee` equals upstream `NOTICES.md` and the canonical copy.
- nix 0.31.3: MIT, MSRV 1.69 in the published manifest.

Publication checks use `version.created_at` from the age file. The paired substitution of bitflags' crate-object `created_at` `2015-01-15T08:35:24.901276Z` failed with exit 8. The age file's `second_created_at` is not the publication input: `parse_age` keeps it and `check_inputs_agree` rejects a publication that equals that second timestamp.

## Governance shape

`governance/qa_record.py` `audited_record_errors` was called on the five on-disk records with `as_of` `2026-10-02T12:12:14Z`. No new Python file. `--write-evidence` was not passed. Each file: `disk_audited=False`, one error, `audited record flags`. An in-memory `audited=true` returned zero `audited_record_errors`. `template_errors` on the filled records returned 26, and 25 for cfg_aliases. That is the blank-template checker.

The record hashes after that call match the hashes after the container run. See `governance/audited-record-errors.txt`.

Zero errors on the flipped memory value is schema shape. The function does not open license bytes and does not read `open_gates`. All ten gates on disk are `open`: named_owner, security_exception, protected_ci, runsc, image, whole_graph, secret_rotation, release, full_at_sec_05, platform_c_library. `admission`, `release_signoff`, and `full_at_sec_05` are false. `license_record_status` is `complete-for-review`. `do_not_treat_as_baseline` is true.

## Unsigned

AT-SEC-05, full SP00, the whole graph, image, owners, platform libc, runsc, CI, secrets, customer, and release stay unsigned. The checker is a record comparator. It is not a scanner, a legal owner, a distribution bundle, or a corporate approval.
