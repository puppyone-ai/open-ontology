# a087 review — fixed main PostgreSQL source integration

Decision: **request_changes**.

Scope of the request: the merged connector's session identity read. A login role with `ALTER ROLE ... SET role` and a caller private CA completes TLS, then `pg_stat_ssl` returns no row for that backend, and `PgSession::read_identity` fails the connect as `Tls` / `tls_not_negotiated`. `inspect_access` never runs, so the intended `role_mismatch` / `session_user` block and the catalog `session_user_mismatch` refusal are not produced. The attempt does not register and does not read the catalog. Fail-closed holds. The classification is wrong, and a normal server-side role GUC cannot be reported as an identity mismatch.

The rest of this slice holds and is not part of the change request: archive identity, the six-file vendor delta, no foundation overwrite from `4213134d136ff50bd2f7d422f3f840e48e038b6d`, SDK pins, `unsafe_code` forbid, the private-CA feature guard, the positive no-roots build, live PostgreSQL 17.10 catalog behavior, the dependency delta, and the full license bodies.

This review does not sign SP03, PostgreSQL 13 or 14, the bounded reader, type conversion, CDC, Loader, publishing, Gateway, CLI, runtime, customer data, CI, runsc, release, or the supply-chain gate. Catalog tests passing do not sign the vertical.

## Pin

Acceptance input is `git archive` of `2980e85af22c3ade05272c66c7d2835b52730322`. Tree `c027c4599cdebf701b21e97e51ded45b9067ca8e`. Parent `1868ffbca151c64a21efd6d1a09cb4806b1e95f3`. All three match `git rev-parse`. Archive file count 3260 equals `git ls-tree -r`. Worktree HEAD at review time was `77d957750e18c7e811e845a7717d54372b41839e`, an ancestor check only. The moving worktree and its dirty files were not the acceptance tree.

`a083` report and HANDOFF text that says the delivery SHA is "this commit" is not an explicit full SHA. The pin above is the identity that was reviewed. Root write ownership stayed with the still-live `a083` handle. This review wrote only under this evidence directory and owned `/tmp` and fixture resources.

## What was run

Image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, platform `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)`, host `aarch64-unknown-linux-gnu`, cargo `1.94.0 (85eff7c80 2026-01-15)`, active because `RUSTUP_TOOLCHAIN=1.94.0`. Kernel `6.12.76-linuxkit`. Debian 13. One owned builder at a time. No host cargo. PostgreSQL image `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb` (17.10 bookworm arm64), one owned fixture, bridge network, no published ports, no host network, no privileged mode, no socket mount, no `HOME` mount, cap-drop `ALL` and `no-new-privileges` on the rust containers. Process listings were pid, ppid, etime, and comm.

`cargo fetch --locked` had no `-p`. Tests and Clippy after that fetch used `--locked --offline`.

First smoke command: `bash /inside.sh` in `oo-a087-msrv-227a9269-f27d-4d99-93cd-e34b64f018d9`. Input SHA-256 of that script: `d9021dc78dbe152802f9ddf5ee0878e91df8da00e0f1f252deacf74103f050c1`. Container exit 101. `oom=false`.

Follow-up, same cargo and target volumes, PostgreSQL log mounted read-only: `bash /followup.sh` in `oo-a087-msrv-227a9269-f27d-4d99-93cd-e34b64f018d9-follow`. Script SHA-256 `ef605a2d02c5e786169a111105ace89d8649bf418b9017cfa1673ef740bb7853`. Container exit 0 because the script records phase exits and does not propagate them. Phase exits below are the ones that count.

| Phase | Exit |
| --- | --- |
| clippy component | 0 |
| fetch | 0 |
| oo-connectors `--lib` | 0 |
| oo-connectors `--doc` | 0 |
| clippy `-D warnings` `--all-targets` | 0 |
| oo-sdk `--lib --tests` | 0 |
| smoke-worst | 0 |
| tree sqlx-core | 0 |
| tree webpki-roots | 101, stderr `package ID specification webpki-roots did not match any packages` |
| tree webpki-root-certs | 0, stderr `nothing to print` |
| tree rustls-native-certs | 0, edge is `oo-sdk` / `reqwest` / `rustls-platform-verifier` |
| tree rustls-platform-verifier | 0, same SDK edge |
| tree sqlx-macros `-p oo-connectors` | 0, `nothing to print` |
| positive `_tls-rustls-ring-no-roots` | 0, class `compiled` |
| `root_store_starts_empty` | 0, `1 passed` |
| first `live_pg` | 101 |
| follow-up `live_pg` | 0 |
| first oracle | 101, compile |
| follow-up oracle | 101, runtime |
| first `cargo deny check --manifest-path` | 2, unexpected argument |
| follow-up `cargo-deny 0.20.2 --manifest-path /src/Cargo.toml --config /src/ci/deny.toml check` | 4 |
| first script `final.exit` | 101 |

`probes.exit` is 1. Every forbidden core feature, its union with `_tls-rustls-ring-no-roots`, and every facade TLS alias except `tls-none` is class `guard`, exit 101, and the stderr contains `Open Ontology patched sqlx-core rejects`. `native-tls` and `webpki-roots` on the core probe retried online after an offline resolution miss and still failed on that guard. `tls-none = []` in official sqlx 0.9.0 compiled, exit 0, class `compiled`. The harness treated that success as a guard miss. Compiling `tls-none` is the right result for an empty feature. It is not a root store.

The selected `cargo tree -e features -i sqlx-core` shows `_tls-rustls-ring-no-roots` and `_rt-tokio` from `oo-connectors`. It does not show `webpki-roots` or `rustls-native-certs` on sqlx-core. SDK `rustls-platform-verifier` and `rustls-native-certs` stay on the Gateway/reqwest edge. Their presence is not an SQLx root store.

The first `live_pg` panic is `postgres log: Os { code: 2, kind: NotFound }` at `audit_log`, because the rust container did not mount `/pglog`. The follow-up mounted `oo-a087-pglog-227a9269-f27d-4d99-93cd-e34b64f018d9` read-only and `live_source_catalog` passed. Printed audit at that moment: `ROLLBACK=72,SELECT=1680,START=72,WITH=96`.

## New case

Fixture role, created before `log_statement=all`: `oo_src_guc_login` LOGIN NOINHERIT, `GRANT oo_src_side` with INHERIT false and SET true, `ALTER ROLE oo_src_guc_login SET role = 'oo_src_side'`. Server `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num` 170010.

Direct verify-full query as that role, app name `oo-a087-sslprobe`, returned `oo_src_guc_login|oo_src_guc_login|oo_src_side|null|null` for login name, `session_user`, `current_user`, `pg_stat_ssl.ssl`, and `pg_stat_ssl.version`. The same query as `oo_src_reader` returned `oo_src_reader|oo_src_reader|oo_src_reader|true|TLSv1.3`. Recorded in `pg/ssl-identity.txt`.

The oracle is `oracle/a087_server_guc.rs`, SHA-256 `ac14bf8fec18c0445612b9e2315b153ab94d0407eeac665c6c4fefcd03f9db20`. Rust 1.94 makes `std::env::set_var` unsafe, and `oo-connectors` forbids unsafe, so the first copy failed to compile (12 `E0133` errors). The follow-up parent starts one child with `Command::env`. The child is the process that calls `PgConnectOptions::from_explicit` and connects. Hostile `PGOPTIONS`, `PGSSLMODE=disable`, forged SSL paths, `PGUSER`, `PGDATABASE`, `PGHOST`, `PGPORT`, `PGAPPNAME`, `PGPASSWORD`, and `PGPASSFILE` were set in that child. `from_explicit` left options, username, database, host, and application name empty, and left the port at 5432.

In that same child, the reader connected: registration allowed, `session_user` and `current_user` both `oo_src_reader`, write state `read_only`, trust `caller-ca-only`, mode `verify-full`, TLS `TLSv1.3`, version 170010, admission `tested`. Empty selection and the empty database both hashed `cb2b03b89678ec1fb9db96f116e8e8ad81a81f6b9bbc2594bcb7d941e5234ecc`. The side role was blocked `write_capable:privilege`. Wrong CA and wrong host (`oo-source-wrong`) both returned `Tls:tls_handshake`. The oracle copy's `Cargo.lock` was unchanged.

The GUC connect then panicked:

`SourceError { kind: Tls, fact: Some("tls_not_negotiated"), sqlstate: None }`

`read_identity` treats a null `pg_stat_ssl.ssl` as TLS not negotiated and returns before `identity_aligned` is stored. `connect` uses `?` on that error, so the session is dropped. `session_block_report` would have emitted `role_mismatch` / `session_user` and `registration_allowed: false`, and `catalog` would have returned `Config` / `session_user_mismatch`, only after a session exists. The `role` startup setting is also outside the allow-list (`DateStyle`, `client_encoding`, `TimeZone`, `extra_float_digits`, `application_name`), so a fixed SSL read would still block on `startup_option` as well as `role_mismatch`. That path was not reached.

Author `live_pg` still passes its client `PGOPTIONS` probes, role inheritance, SET-only, definers, operators, casts, and admin-style roles. Those remain prior facts revalidated by follow-up exit 0. They do not cover a server-side `role` GUC.

## SQL audit

`pg/postgresql.log` SHA-256 `4fe7311b5e8c9c1fcf935d62db9d4306c639bff25e8cf7951b01519f8d685ea1`, 1167957 bytes, copied after the oracle and the SSL probe. Twenty-one fixture password files: 0 hits. Comment canary: 0 hits. Bodies were not printed.

Lines for `app=oo-pg-source`: 3386. Prepared-statement and `statement:` verbs on those lines: `SELECT` 1748, `WITH` 102, `START` 77, `ROLLBACK` 77. No `INSERT`, `UPDATE`, `DELETE`, `TRUNCATE`, `CREATE`, `DROP`, `ALTER`, `GRANT`, `REVOKE`, or `COPY`. The live test's smaller counts are the log before the oracle. Other app names present: `oo-a087-ident`, `oo-a087-sslprobe`, `oo-pg-fixture`, `psql`, `[unknown]`.

## Provenance

Compared with `4ecdbf714766baef3490a4f954701eaf19f245a0`, `crates/oo-connectors` differs in exactly six paths:

- `vendor/PATCH.md` (4432 bytes, SHA-256 `39f602dff805742661b5c6915a0634f86718443b8d5ca3f1e2f2cff53b5e206c`)
- `vendor/sqlx-core/LICENSE-APACHE` and `LICENSE-MIT`
- `vendor/sqlx-postgres/LICENSE-APACHE` and `LICENSE-MIT`
- `vendor/sqlx-core/src/lib.rs` (3722 bytes, SHA-256 `03f813d1b4ae006049bc3fc213c6302164bd864d505db54e040ff0c4e586dba9`)

Both license bodies are the VCS texts: MIT `5abbdd842310c8f1650fcde3a5a51a3e09412b9e5a6eea87519e0db9e32b339d` (1109 bytes) and Apache-2.0 `c8f5453612253e8ea8bad241617fe74c4a6d5d592ba64ff881668c211b50ac99` (10297 bytes). Both contain `Copyright`. The 4ec copies are the short dangling pointers. `PATCH.md` is not a `../` pointer.

The four behavior files are byte-identical between 4ec and this pin: `sqlx-core` `Cargo.toml` `230c8c8e799f06744977d9dca7ef7f2f1cf1ea4402c7c396cadff172c955a2e1`, `Cargo.toml.orig` `8943a4a994760595ebeb229e0d82847df70b3cc05b48c3eca642b4c36694ff0e`, `src/net/tls/tls_rustls.rs` `cbc9fe35bb4b845eccf0b5ff16c45d620c3b9e8f75d9b842e2569a484507eafe`, `sqlx-postgres` `src/options/mod.rs` `127a4a8de11f34d1930e32d81447d28573beddee5c8252e69d50c81acc894499`. Official 0.9 file counts stay 110 and 119. No added or removed vendor paths. Integration versus 4ec inside the two crates is licenses plus `sqlx-core/src/lib.rs`.

Independent `git diff --no-index --no-renames` of those four files, official tree against the 4ec vendor tree, is 4311 bytes, 112 lines, SHA-256 `0129343a96be05b31dabc2824be8b2d587ac43d4eddde656c4dcad17f6c4948e`. That matches a083's stored no-index diff, regenerated here from the crates.io tarballs and `git show` of 4ec. The hunk body, with git headers removed from the comparison, is SHA-256 `1336538d26ec757710cb5c0dd91a3eac5102cecac3ede6363c8c19fb7d783b4a` (3443 bytes) and matches a083's hunk body. a074's `f4b1aff1f2575a6b161a5cec37da26a4e097475aa1dcde3a9cec603766ac5f37` (3664 bytes, 104 lines) was not reproduced as a byte stream. Dropping `diff --git` and `index` lines from the regenerated diff leaves 104 lines and 3811 bytes. The line count matches a header-stripped unified diff. The byte length differs by path prefixes. The four-file hunks are the same edit. Git 2.55.0's directory `diff --no-index` inserts `similarity index` lines; per-file `--no-renames` is what reproduced `0129343a`.

Official tarball SHA-256 values match the published checksums: sqlx `378620ccc25c62c89d8be1c819e76a88d59bdcc3304733330788948e619bfd71`, sqlx-core `05b44e85bf579a8eeb4ceaa77a3a523baf2bf0e9bac7e40f405d537b5d2d5ccb`, sqlx-postgres `87a2bdd6e83f6b3ea525ca9fee568030508b58355a43d0b2c1674d5f79dcd65e`. Both patched crates carry `.cargo_vcs_info.json` git SHA-1 `003b698e99e024f3621b8043a2426fde5b741171` and `path_in_vcs` `sqlx-core` / `sqlx-postgres`. The root lock uses path identity for those two crates (`checksum` null). That is the declared modified-upstream patch, not a registry checksum change.

Root `Cargo.toml` versus the parent adds only the `[patch.crates-io]` block for those two paths, with a comment that they are modified upstream artifacts. `crates/oo-connectors/Cargo.toml` SHA-256 `b9d8c7bfda4c563994024190ab0ad73c9fffefe013625a760aa47bc1f483216d` equals 4ec and the coordinator observation. Parent skeleton hash is `9ed9bf53c764f72a759418d69665ffafd9ee4e5f3383e4f5dc7e9cd26d322b3d`. `a081` overwrote and restored the live manifest while `a083` was working. This commit does not contain those intermediate bytes. The fixed manifest, the compiled graph, and the rebuilt fixture are what this review can prove. The missing intermediate timing is a residual, and it is not a byte mismatch in the delivered manifest.

`testing/product/pg-source` is six paths added versus the parent and byte-identical to 4ec. A provenance allow-list omitted that prefix and listed them as outside the allowed set. Foundation, xtask, core, builder, pipeline, jobs, SDK, CLI, and Gateway paths are unchanged versus the parent. SDK paths since `8efeb230afc6734a2e50eaca3f09fd78300a9c7d` are empty. `unsafe_code = "forbid"` is in the root workspace lints, `oo-connectors` `src/lib.rs`, and patched `sqlx-core` `src/lib.rs`.

## Dependencies

Lock SHA-256 `9b8f7e50d3b20042bb2d606f47276b76dd68f3568136343040fbea1ecd622057`. Parent and SDK lock SHA-256 `62bdbb234e527452c6d4313fcab18948d3b017dfccaca545b5d165c0ff4bcde3`. Base `4213134` lock SHA-256 `636e19b8761bd4f212381fd0abe6a3040ca6ee8024aed06dd350a1fd55ec32d1`. Packages 267 to 305: 38 added, 0 removed. `same_version_source_or_checksum_changes` is empty. Names are in `provenance/dependency-delta.json`.

Age cutoff used by the checker: `2026-09-25T02:15:37Z`. All 38 have `age_ok` true and `yanked` false. Registry checksums match the lock except `sqlx-core` and `sqlx-postgres`, whose lock checksum is null because they are path crates. Metadata `rust_version` is absent for `crc-catalog` 2.5.0, `futures-intrusive` 0.5.0, `hex` 0.4.3, `stringprep` 0.1.5, and `unicode-properties` 0.1.4. The 1.94.0 linux/arm64 compile of this lock is the selected-graph evidence. sqlx, sqlx-core, sqlx-macros, sqlx-macros-core, and sqlx-postgres publish `rust_version` 1.94.0.

`cargo-deny` 0.20.2, tarball SHA-256 `995c82be0defc7a025cae49a2aa2644ce8245c9a3318fc4103907c6a285e8c7d`, on `ci/deny.toml`: `advisories ok, bans ok, licenses FAILED, sources ok`, exit 4. The only rejection is `webpki-root-certs` 1.0.9 `CDLA-Permissive-2.0` through `rustls-platform-verifier` and `oo-sdk`. Host `cargo tree -i webpki-root-certs` printed nothing. Deny still sees the package. That CDLA edge, MIT/Apache-2.0 slash strings (`stringprep`, `unicode-properties`), `whoami` `Apache-2.0 OR BSL-1.0 OR MIT`, and native conjunctive obligations stay separately scoped. This review does not widen `ci/deny.toml` and does not treat a host-unselected package as absent from the distributable graph. ISC is already in the allow list on this pin, so the older ring/webpki/untrusted ISC failure from a074's stricter file did not recur.

## Author evidence

Rehashed, not rewritten. `0\n` is `9a271f2a916b0b6ee6cecb2426f0b3206ef074578be55d9bc94f6f3fe3ab86aa`. `1\n` is `4355a46b19d348dc2f57c046f8ef63d4538ebb936000f3c9ee954a27460dd865`. `101\n` is `39b8dc3fc8b44765c8e6f1adee04c5b465e555ab791cc42d0d9e810d5b64297c`.

Preserved integers: `baseline/phases/test.exit` 101, `baseline/phases/worst.exit` 101, `baseline/docker-run.exit` 101, baseline `test-lib` and `test-doc` 0. First integration `linux/phases/clippy.exit` 1, `linux/phases/sdk-test.exit` 101, `linux/phases/worst.exit` 101, `linux/docker-run.exit` 101. Live rerun `linux-live/phases/live.exit` 0, `live-worst.exit` 0, `linux-live/docker-run.exit` 0, while `linux-live/phases/worst.exit` stays 101 because the guard probes are nonzero. Author PostgreSQL log SHA-256 `8985b98a354cc8913218c682ad1bb25e8072b99915aac7a518c56ef91bb05dc4` matches. Both license bodies match the VCS hashes above.

## Cleanup

All exit 0: stop and rm of both rust containers and the PostgreSQL container, `docker network rm oo-a087-net-227a9269-f27d-4d99-93cd-e34b64f018d9`, volume rm of the cargo, target, and pglog volumes, and removal of `/tmp/oo-a087-227a9269-f27d-4d99-93cd-e34b64f018d9` plus the patch-identity work directories. No prune. No other containers were running afterward. Recorded in `cleanup/exits.txt`.

## Residuals that are not the change request

- `a081` live manifest bytes between overwrite and restore are not in this commit.
- CDLA on `webpki-root-certs`, slash-form MIT/Apache licenses, and the whoami BSL alternative remain policy work outside this slice.
- Full dual-arch, release, and supply-chain sign-off were not run.
- The first harness missed the log mount, called `set_var`, and passed `--manifest-path` to `cargo-deny check`. Those exits are recorded and were rerun.
