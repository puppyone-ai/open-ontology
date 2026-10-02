# a078 review: ISC attribution correction

Decision: **approve** for the attribution wording in this slice. The secret gate is not accepted. This review does not approve SP00-W06, `CDLA-Permissive-2.0`, reconciliation of the OR-token mismatch, protected CI, TLS, customer delivery, or release.

Reviewed commit `fb2d1d19c21b510dfd6231c3b67d0efbf4285491`, tree `79f19b818d3e14ad08ad5208ce2bd0a17b4c297e`, parent `6fa6481267e5a4efac20831fa0b407a8cf028905`. `git rev-parse` returned those three values. At the start of this review, `git merge-base --is-ancestor` of that commit against HEAD exited 0. HEAD was `304fcf9f116bbbabe0f91a2caca5df5949ef476c` on `master`. The review used `git archive` of that commit. It did not check the commit out, and it did not edit source, `ci/deny.toml`, contracts, locks, or a067 evidence.

a069 (`eef831aa-dba4-4af2-a78b-541bc9e0a069`) requested one wording correction: `r-efi` 6.0.0 contains the MIT grant in `AUTHORS`. This commit makes that correction and leaves the ISC allow entry in place. cargo-deny was not reinstalled and the `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74` graph was not run again. a069 already recorded that graph. No new license-rule finding here required a repeat.

## What the diff is

`git diff --name-only` from the parent exits 0 and lists 52 paths. Every path is under `contracts/supply-chain/isc/` or `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/`. The diff does not touch `ci/deny.toml`, `Cargo.toml`, `Cargo.lock`, `supply-chain/policy.md`, product code, or the a067 evidence tree.

Blob identities are the same on the parent and on this commit:

| Path | Blob |
|---|---|
| `evidence/.../e0a067` tree | `77509f0fb3fb72356101c10082366cc3da3700ce` |
| `contracts/supply-chain/isc/fixtures` tree | `f06b77c0ec3cfddc4cace9890a9e682cfe104e15` |
| `ci/deny.toml` | `367598cb3768b8c8af698041cd0f84ce6f8fb7be` |
| `supply-chain/policy.md` | `746a8054261147b64567873b26c906791bca84a1` |
| `Cargo.lock` | `b30c565888a98d1ac0eb99057e66ebf5b3c5ccd2` |
| `Cargo.toml` | `1dd71ee45257f548346d1a221dee280466546558` |

`ci/deny.toml` still has one `"ISC"` allow entry, after `"MIT"`. The allow list is Apache-2.0, MIT, ISC, BSD-3-Clause, Unicode-3.0, Zlib, Unlicense, MIT-0. The file contains no `CDLA` string.

The working tree's `Cargo.lock` was dirty at the end of this review (`git diff --numstat` reported 1446 insertions and 118 deletions). This review did not run host cargo and did not write that file. The bytes under review are the commit blob above. The dirty worktree lock was left as it stood.

## Upstream bytes

Six GETs exited 0. Crate files came from `https://static.crates.io`. Version rows came from `https://crates.io/api/v1/crates/<name>/<version>`. User-Agent was `oo-a078-attribution-review (local byte check)`. sha256 of each crate file matches the API `checksum` and `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74:Cargo.lock`. None of the three versions is yanked.

| Crate | Bytes | sha256 | API `created_at` | SPDX | In this commit's root lock |
|---|---|---|---|---|---|
| r-efi 6.0.0 | 65303 | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` | 2026-02-03T09:53:46.251122Z | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | no |
| r-efi 5.3.0 | 64532 | `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f` | 2025-06-17T09:12:38.227277Z | `MIT OR Apache-2.0 OR LGPL-2.1-or-later` | yes, same checksum |
| rustls 0.23.45 | 378933 | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` | 2026-09-14T15:11:17.808465Z | `Apache-2.0 OR ISC OR MIT` | no |

Each r-efi crate has 69 files. Neither has a file whose basename is `LICENSE`, `COPYING`, or `NOTICE`. rustls 0.23.45 has 119 files and likewise has no file with those three basenames. Its license texts are `LICENSE-APACHE`, `LICENSE-MIT`, and `LICENSE-ISC`.

The committed body copies match the extracted crate bytes, and the a077 `raw/upstream` copies match those same bytes.

## AUTHORS-MIT, copyright lines, and the two pointers

`r-efi` 6.0.0 `AUTHORS` is 3733 bytes, sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111`. The file order is the short `LICENSE:` header, `AUTHORS-MIT`, `AUTHORS-ASL`, `AUTHORS-LGPL`, `COPYRIGHT:`, then `AUTHORS:`.

`AUTHORS-MIT` is the MIT grant. It begins `Permission is hereby granted, free of charge` and includes `The above copyright notice and this permission notice shall be included`.

The copyright lines are below that grant:

```
        Copyright (C) 2017-2023 Red Hat, Inc.
        Copyright (C) 2019-2023 Microsoft Corporation
        Copyright (C) 2022-2023 David Rheinsberg
```

The name list follows those lines. 6.0.0 has 17 names. `AUTHORS-ASL` is the short Apache header. It points at `http://www.apache.org/licenses/LICENSE-2.0`. It does not contain `Grant of Copyright License` or the Apache appendix. `AUTHORS-LGPL` is the short LGPL notice. It points at `http://www.gnu.org/licenses/`. It does not contain `TERMS AND CONDITIONS` or `0. Additional Definitions`. Those two sections are pointers. They are not the full Apache-2.0 or LGPL texts.

`r-efi` 5.3.0 `AUTHORS` is 3693 bytes, sha256 `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7`. It has the same grant, the same three copyright lines, and 16 names. The only line present in 6.0.0 and absent in 5.3.0 is `Alan Egerton <eggyal@gmail.com>`.

Both `README.md` files are sha256 `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1` and are byte-identical. The README contains the markdown line `**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**`. It does not contain the MIT grant.

`NOTICE.md`, `policy.md`, `provenance.md`, and `ERRATUM.md` on this commit state the grant, the three copyright holders, the name list, the Alan Egerton difference, both versions, and the Apache/LGPL pointer distinction. The current notice and policy do not repeat the old claim that the crate has no MIT text or that `AUTHORS` is not that license.

## Rustls ISC comma

`LICENSE-ISC` is 775 bytes, sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4`. Line 2 is `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`. `LICENSE-MIT` line 1 is `Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>`, with no comma. The erratum and the current notice quote the comma. The a067 report still prints the gloss without it.

## Historical record and the erratum

The a067 tree hash is unchanged. The a067 report still says `` `r-efi` 6.0.0 is the real crate with that gap. `` The a067 handoff still says the missing-text fixture "is the `r-efi` 6.0.0 situation, and it is not proof the MIT text shipped." The parent `NOTICE.md` and `policy.md` still contain the sentences the erratum quotes, including "`AUTHORS` is not that license." and "the synthetic form of the `r-efi` fact."

`ERRATUM.md` opens with "This file supersedes the statements quoted below." It quotes those sentences and the rustls comma line. It points at `raw/parent.txt` for its parent and does not name its own commit object. That file contains `6fa6481267e5a4efac20831fa0b407a8cf028905`, which is the parent of `fb2d1d19`.

`fixtures/mit-or-gpl-missing-mit-text` is byte-identical to the parent tree. Its `LICENSE` still says the synthetic package ships neither the MIT text nor the GPL text. The current policy calls that fixture an expression-only probe and says it is not the `r-efi` crate.

## What stays pending

`supply-chain/policy.md` still says GPL-2.0 and GPL-3.0 stay out of the default artifact even when an `OR` expression also names a permissive license, and that a prohibited token blocks the default record even as one side of an `OR`. That blob is unchanged. `ci/deny.toml` still has no token scan. The ISC policy text says the mismatch stays explicit and separate pending work and does not weaken either rule. This review leaves that mismatch pending.

Ring remains `Apache-2.0 AND ISC` in the erratum, the ISC policy, and the provenance table. The notice says both Apache-2.0 and ISC apply. `CDLA-Permissive-2.0` is not in the allow list. The notice still says `webpki-roots` 1.0.9 is not allowed. The erratum says SP00-W06, protected CI, TLS, source registration, customer delivery, and release stay unapproved. This review does not reopen those gates.

## Linux arm64 Rust 1.94 smoke

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, local id the same digest, architecture arm64, OS linux. `--pull never`, `--platform linux/arm64`. `RUSTUP_TOOLCHAIN=1.94.0`. Package `oo-core`, because the commit diff has no Rust sources. Fetch had no `-p`. Test and Clippy were offline after fetch. This is one package on one architecture. It is not a matrix and not an air-gap claim.

Before attempt 1, running rust-image or `oo.kind=rust-builder` containers were 1 (`oo-a076-msrv-81b3f22f`, image rust 1.94, configured command `sleep`, left running). Own rust builders were 0. Host `cargo`, `rustc`, `rustdoc`, and `rustup` comms were 0. Attempt 2 started later, when that count was 0 and own was 0. Capacity listings used pid, ppid, etime, and comm only.

Attempt 1 snapshot was `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `crates`, and `.cargo`. Archive `Cargo.lock` blob `b30c565888a98d1ac0eb99057e66ebf5b3c5ccd2` matches the commit. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, `cargo 1.94.0 (85eff7c80 2026-01-15)`. Clippy was absent and `rustup component add clippy --toolchain 1.94.0` exited 0. Fetch exited 0 with 119 `Downloaded` lines before the fetch marker and 0 after it. `cargo test -p oo-core` exited 101. `fixtures_pg_mysql.rs` `fixture_file_says_no_driver_was_used` panicked with `NotFound` at `crates/oo-core/tests/fixtures_pg_mysql.rs:41` because the snapshot omitted `contracts/fixtures/pg-mysql/cases.json`. Unit tests 11 passed, `boundary_corrections` 11 passed, `canonical_golden` 8 passed, and that fixture target had 4 passed and 1 failed. Cargo stopped there, so `frozen_corrections` and `schema_contract` did not run. Clippy then exited 0. Docker run exited 101. `elapsed_seconds=29`. Those exit files were not rewritten.

Inspect of attempt 1: privileged false, user `0:0`, CapDrop `ALL`, CapAdd null, security `no-new-privileges`, memory and memory-swap `8589934592`, NanoCpus `4000000000`, pids limit 4096, network `oo-a078-4ebcaafc-8442-4e95-9706-59ca03b0b1cb-net`, OOMKilled false. `HostConfig` has no `Tmpfs` key. Mounts were the archive read-only, `inside.sh` read-only, the attempt evidence directory, and task-owned cargo and target volumes. `HOME=/tmp` inside the container. No docker socket and no host network. `HostConfig.Binds` is empty because the run used `--mount`. The mount list is the record of the binds.

Attempt 2 used the same commit and added `contracts` and `examples` to the archive. The same toolchain string was printed. Phase exits, all from the attempt-2 files: toolchain 0, snapshot 0, extra inputs 0, clippy component 0, fetch 0, test 0, clippy 0, docker run 0, inspect 0, bookkeeping 0. Fetch again had 119 downloads before the marker and 0 after. Tests: lib 11, `boundary_corrections` 11, `canonical_golden` 8, `fixtures_pg_mysql` 5, `frozen_corrections` 19, `schema_contract` 8, doctests 0. That is 62 passed and 0 failed. `elapsed_seconds=31`. Inspect matches attempt 1's isolation, on network `oo-a078-4ebcaafc-8442-4e95-9706-59ca03b0b1cb-a2-net`. `Tmpfs` is absent there too.

## Secret residual

An actual local exposure happened. The committed replacement for `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/pre-smoke/processes-before.txt` is 173 bytes and 5 lines. Its longest alphanumeric run is 11. Credential-shaped counts on that blob are 0. The worktree file matches the committed bytes. The text is:

```
Redacted before publication. The first capture used ps command lines and included local credentials.
This file keeps the capacity result only.
builders=0
own=0
host_cargo=0
```

The a077 `launch.sh` still contains the argv `ps -ax -o pid,comm,command`. That is the capture method. The committed output of that capture is the replacement above.

Current object store: `git cat-file --batch-all-objects` scanned 8634 objects (2410 trees, 6191 blobs, 33 commits). 6181 text blobs were scanned, 10 binary blobs were skipped, 0 blobs were over 20 MiB. Pattern totals for a Cursor-style key prefix, a `crsr_` prefix, a WorkOS session-cookie name, a WorkOS user-session shape, a JWT, a Cursor API assignment, a bearer token, and a long mixed-case field beside those keywords were all 0. `git cat-file` exited 0.

`git fsck --no-progress` exited 0. It reported 32 dangling trees and 0 dangling blobs, commits, or errors. `git fsck --connectivity-only --no-progress` exited 0 and reported the same 32 dangling trees. `git count-objects -v` reported 742 loose objects, 7892 packed objects, 4 packs, and 0 garbage. Loose and packed blobs were both in the content scan. The dangling trees were not deleted.

The evidence tree and `/tmp/oo-a077*` / `/tmp/oo-a078*` files were scanned the same way. One hit remained: `/tmp/oo-a078-4ebcaafc-8442-4e95-9706-59ca03b0b1cb/scan_exposure.py`, the probe itself, whose source contains the session-cookie pattern label. That file was a reviewer probe and was removed with the probe directory. It is not repository content.

Local remote-tracking branches that contain `fb2d1d19` : none. No fetch and no push were performed, so the remote itself was not checked.

Four separate facts:

1. Current committed evidence and the local object store, under this scan, do not contain the credential shapes above.
2. Historical local exposure happened. The author's first process-list capture included a Cursor API key and session tokens. The committed output was replaced. The author says the offending loose objects were removed and the capture was not pushed.
3. Unverified copies remain outside this inspection: credential stores, shell history, Cursor application data, agent session transcripts, foreign container logs, backups, and any remote whose tracking ref is stale. Those places were not read.
4. The holder still needs to rotate that Cursor API key and those session tokens. A clean current tree does not show that every copy is gone, and it does not retire a key that was already displayed.

The secret gate is not accepted.

## Commands and exits

| Command | Exit |
|---|---|
| `git rev-parse fb2d1d19c21b510dfd6231c3b67d0efbf4285491` and `^{tree}` and `^` | 0, values above |
| `git merge-base --is-ancestor fb2d1d19 HEAD` | 0 |
| `git diff --name-only 6fa64812 fb2d1d19` | 0, 52 paths |
| `python3 raw/verify_attribution.py` | 0 |
| six upstream GETs inside that script | 0, 0, 0, 0, 0, 0 |
| `python3 raw/scan_exposure.py` | 0 |
| `git fsck --no-progress` | 0 |
| Attempt 1 `docker run` / fetch / test / clippy / clippy-component / toolchain / snapshot | 101 / 0 / 101 / 0 / 0 / 0 / 0 |
| Attempt 1 bookkeeping | 0 |
| Attempt 1 `docker rm`, `docker volume rm` (cargo, target), `docker network rm` | 0, 0, 0, 0 |
| Attempt 2 `docker run` / fetch / test / clippy / clippy-component / toolchain / snapshot / extra-inputs / inspect / bookkeeping | 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 / 0 |
| Attempt 2 `docker rm`, volume rm, network rm, `rm -rf /tmp/oo-a078-4ebcaafc-8442-4e95-9706-59ca03b0b1cb` | 0, 0, 0, 0, 0 |

After cleanup this task owned 0 containers, 0 volumes, and 0 networks. No prune. a073, a070, a075, and a072 containers were not stopped or removed. The reviewer did not commit.
