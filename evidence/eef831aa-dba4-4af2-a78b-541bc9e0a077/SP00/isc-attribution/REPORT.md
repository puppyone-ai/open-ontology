# a077 ISC attribution correction

Session `eef831aa-dba4-4af2-a78b-541bc9e0a077`. Run `e844e982-ba5c-462c-b01a-27d3c63bf811`.

The parent of this commit is `raw/parent.txt`, written from `git rev-parse HEAD` immediately before `git commit`. This report does not name its own commit object.

a069 (`eef831aa-dba4-4af2-a78b-541bc9e0a069`) requested one wording correction: `r-efi` 6.0.0 does contain the MIT grant, in `AUTHORS`. The ISC allow entry stays. `CDLA-Permissive-2.0` stays off the allow list. This commit does not stop other product work and does not approve SP00-W06, protected CI, TLS, source registration, customer delivery, or release.

## What changed

Owned paths:

- `contracts/supply-chain/isc/NOTICE.md`
- `contracts/supply-chain/isc/policy.md`
- `contracts/supply-chain/isc/provenance.md`
- `contracts/supply-chain/isc/ERRATUM.md`
- `contracts/supply-chain/isc/bodies/r-efi-6.0.0/AUTHORS`
- `contracts/supply-chain/isc/bodies/r-efi-5.3.0/AUTHORS`
- `contracts/supply-chain/isc/bodies/rustls-0.23.45/LICENSE-ISC`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/`

`ci/deny.toml`, root `Cargo.toml`, root `Cargo.lock`, `supply-chain/policy.md`, the synthetic fixtures, product code, docs, the spec, the ledger, and the a067 evidence were not edited. The a067 `REPORT.md`, `HANDOFF.md`, and `raw/` stay the historical record. [ERRATUM.md](../../../../contracts/supply-chain/isc/ERRATUM.md) quotes the superseded sentences.

## Verified statements

Downloaded from `https://static.crates.io` on this run. Crate sha256 matches the file bytes, the crates.io API `checksum`, and `b2d3cbc5af2b97d6565cbd67f0a9208f778a8c74:Cargo.lock`. None of these three versions was yanked.

| Crate | sha256 | API `created_at` |
|---|---|---|
| r-efi 6.0.0 | `f8dcc9c7d52a811697d2151c701e0d08956f92b0e24136cf4cf27b57a6a0d9bf` | 2026-02-03T09:53:46.251122Z |
| r-efi 5.3.0 | `69cdb34c158ceb288df11e18b4bd39de994f6657d83847bdffdbd7f346754b0f` | 2025-06-17T09:12:38.227277Z |
| rustls 0.23.45 | `0d41d731c7d2f962d1ccc364cec258de3c0e93b38c2fb3ba97ac74513048d634` | 2026-09-14T15:11:17.808465Z |

The current root `Cargo.lock` pins r-efi 5.3.0 at that checksum. It does not pin r-efi 6.0.0. Both versions are in the b2 lock.

`r-efi` 6.0.0 `AUTHORS` is 3733 bytes, sha256 `d027e91dbc9cdbb2f1190068e498bd6b61cff022b6a032b191021ba658d96111`. It contains the MIT grant under `AUTHORS-MIT`, including `Permission is hereby granted, free of charge` and `The above copyright notice and this permission notice shall be included`. The copyright lines below the grant are Red Hat, Inc. (2017-2023), Microsoft Corporation (2019-2023), and David Rheinsberg (2022-2023). An `AUTHORS:` name list follows. `AUTHORS-ASL` points at `http://www.apache.org/licenses/LICENSE-2.0`. `AUTHORS-LGPL` points at `http://www.gnu.org/licenses/`. Those two sections are pointers, not the full Apache-2.0 or LGPL texts.

`r-efi` 5.3.0 `AUTHORS` is 3693 bytes, sha256 `ff92bed461f50338dd703a9ba9aee496a425957873df1d91192776e4bdf5dda7`. The only line in 6.0.0 that is absent from 5.3.0 is `Alan Egerton <eggyal@gmail.com>`. Both `README.md` files are sha256 `420f65fec3ba2e89d49da51b222d2881d26959037a7b7d3794143db4bd4627b1`. The README license line is markdown `**MIT** OR **Apache-2.0** OR **LGPL-2.1-or-later**`. The README does not contain the MIT grant.

Neither crate has a file named `LICENSE`, `COPYING`, or `NOTICE`. Each tree has 69 files.

rustls `LICENSE-ISC` is 775 bytes, sha256 `7cfafc877eccc46c0e346ccbaa5c51bb6b894d2b818e617d970211e232785ad4`. Its second line is `Copyright (c) 2016, Joseph Birr-Pixton <jpixton@gmail.com>`. `LICENSE-MIT` begins `Copyright (c) 2016 Joseph Birr-Pixton <jpixton@gmail.com>`, with no comma. The a067 report omitted the ISC comma. That report was not edited.

`fixtures/mit-or-gpl-missing-mit-text` is unchanged. Its `LICENSE` still says the synthetic tree ships neither text. It remains an expression-only cargo-deny acceptance probe. It is not a description of the `r-efi` body.

Ring stays `Apache-2.0 AND ISC`. The ring notice text in `NOTICE.md` is unchanged. `"ISC"` remains in `ci/deny.toml`. `CDLA-Permissive-2.0` was not added. The OR-token mismatch with `supply-chain/policy.md` §2 and §3 stays separate pending work.

cargo-deny was not reinstalled. The b2 deny graph was not re-run. a069 already recorded that graph.

## Pre-edit Linux arm64 check

Archive of `6fa6481267e5a4efac20831fa0b407a8cf028905`, tree `74de1cad08f0c84fb53ca365fc5b4bdac98df40a`, parent `8e5d132ab3d5888017fae9db51de4125deb969f4`. Workspace inputs only, via `git archive`. Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `--pull never`, `--platform linux/arm64`. Container `oo-a077-msrv-e844e982-ba5c-462c-b01a-27d3c63bf811`.

`RUSTUP_TOOLCHAIN=1.94.0`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`. `cargo 1.94.0 (85eff7c80 2026-01-15)`.

| Phase | Exit |
|---|---|
| toolchain is 1.94.0 | 0 |
| snapshot has the cargo inputs | 0 |
| `cargo fetch --manifest-path Cargo.toml --locked` | 0 |
| `cargo check --manifest-path Cargo.toml --locked --offline -p oo-core` | 0 |
| `docker run` | 0 |

119 `Downloaded` lines, all between the fetch marker and the check marker. None after the check marker. `elapsed_seconds=13`. Check finished `dev` profile in 8.67s. This is one package on one architecture. It is not a matrix and not an air-gap claim.

Inspect of the exited container: privileged false, user `0:0`, CapDrop `ALL`, CapAdd null, security `no-new-privileges`, memory and memory-swap `8589934592`, NanoCpus `4000000000`, pids limit 4096, network `oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811-net`, OOMKilled false. `HostConfig` has no `Tmpfs` key. Mounts were the archive read-only, `inside.sh` read-only, this pre-smoke directory, and task-owned cargo and target volumes. `HOME=/tmp`. No docker socket and no host network. The evidence bind is the task directory. It is not a mount of the host home directory as `HOME`.

Before the run, rust-builder count was 0 and host `cargo`/`rustc` count was 0. The a072 Postgres container was up and was not a Rust builder.

The launch shell then exited 1. The format string asked for `HostConfig.Tmpfs`, and this Docker API has no such key. `docker-run.exit` was already 0. The phase exit files were not rewritten. `pre-smoke/bookkeeping.txt` records that.

## Checks

```
python3 evidence/eef831aa-dba4-4af2-a78b-541bc9e0a077/SP00/isc-attribution/raw/check-attribution.py
```

Exit 0. Log: `raw/check-attribution.stdout`. The script compares the byte copies with the extracted crate files and the hashes above, checks the old and new sentences, checks that the a067 report and handoff still contain the historical wording, and checks that `ci/deny.toml`, the root lock, `supply-chain/policy.md`, the fixtures, and the a067 evidence have no status diff.

Other agents had uncommitted work under `crates/oo-sdk`, `crates/xtask`, `crates/oo-jobs`, `crates/oo-pipeline`, `docs/adr`, and `governance`. Those paths were not staged.

## Cleanup

Removed container `oo-a077-msrv-e844e982-ba5c-462c-b01a-27d3c63bf811`, volumes `oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811-cargo` and `oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811-target`, network `oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811-net`, and `/tmp/oo-a077-e844e982-ba5c-462c-b01a-27d3c63bf811`. No prune. Afterward this task owned no containers, volumes, or networks. `raw/cleanup.txt` is that log. Foreign containers were left running.
