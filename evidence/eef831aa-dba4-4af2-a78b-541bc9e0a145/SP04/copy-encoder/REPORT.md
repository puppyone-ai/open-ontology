# Report — a145 COPY encoder

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a145`. Slot A. One private Rust 1.94 builder and one private PostgreSQL 17.10. This slice is `copy-encoder/v1`: a pull encoder from one `TypedBatch` and an approved column list to PostgreSQL binary COPY bytes. Full reader admission and full apply stay gated. The ledger was not edited.

Fixed source `6b95d6f2fd2259edfcbffc27a6338518a64dd0fc`, tree `baaabaf1384646237e0c40bfa1914ea150d39c5b`. Owned source commit `d80472d8a99afd66d8d0e71420f7be2bb62e65c8`, tree `a3b10827acfac28c4abf986831cbcb26823477dc`, parent `8e0d1da658d65878be96f6f2a7a4478dd3222b19`. Four paths, 2586 insertions:

- `contracts/pipeline/copy-encoder-v1.md` blob `9b1d8f12be86675f35604bbb2e31d5fa3db57248`
- `crates/oo-pipeline/src/copy_encoder.rs` blob `8eb2224204ea3f6e57d8407ce218bab790a8059d`
- `crates/oo-pipeline/src/lib.rs` blob `2e0fa8e0fe4de6f2d72ae2e7b73cd4881805c556`
- `crates/oo-pipeline/tests/copy_encoder_live.rs` blob `2021732e21dac0ff7a280336c3a5e44fb44238af`

`physical_mapping.rs`, `crates/oo-pipeline/Cargo.toml`, and `contracts/source/pgsql/type-conversion-v1.md` are the fixed blobs. Content sha256 of those three files matches `linux/focused-6/expect-mapping.sha256`, `expect-cargo.sha256`, and `expect-types.sha256`. The four committed files match `expect-contract.sha256`, `expect-encoder.sha256`, `expect-lib.sha256`, and `expect-live.sha256`.

Smoke archived workspace HEAD `dc5196b27eb388007d869495491aa4fef23e214e`. Focused runs archived workspace HEAD `8e0d1da658d65878be96f6f2a7a4478dd3222b19`. Pipeline, connector type, and canonical sources were unchanged against the fixed base at that HEAD. The proof compiled a `git archive` of the fixed commit plus an overlay of the four owned paths. It did not compile the later source commit, because that commit was made after the green run from the same bytes.

## Toolchain and images

Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, `linux/arm64`, `--pull never`. `rustc 1.94.0 (4a4ef493e 2026-03-02)` host `aarch64-unknown-linux-gnu`, LLVM 21.1.8. `cargo 1.94.0 (85eff7c80 2026-01-15)`. `RUSTUP_TOOLCHAIN=1.94.0`. `HOME=/tmp`. Postgres image `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, `linux/arm64`.

Focused-6 hostconfig: `privileged=false`, `capdrop=["ALL"]`, `security=["no-new-privileges"]`, `net=oo-a145-eef831aa-net`, `memory=8589934592`. Postgres hostconfig: `privileged=false`, `capdrop=["ALL"]`, `capadd` only `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETGID`, `SETUID`, `memory=1073741824`, same network. Auth `trust`. No published port, no host network, no docker socket, no host home mount, no shared cargo cache. Containers `oo-a145-focused-6` and `oo-a145-pg`. Volumes `oo-a145-eef831aa-cargo`, `oo-a145-eef831aa-target`, `oo-a145-eef831aa-pg6`.

## What ran

Smoke, before any encoder source: `linux/smoke/`. Archive of `6b95d6f2` with no overlay. `lib.rs` content sha256 `51ae51157e83e9f892e5588031c39bb9592884a198109aa8b996f268cae15ae8`. Exits: toolchain, snapshot, fetch, test, clippy, clippy-component, docker-run all 0. Package test included the seven `physical_mapping` unit tests and the non-ignored integration tests already on that tree. `physical_mapping_live` stayed ignored. Elapsed 38 seconds. Vendored `sqlx-postgres` printed two unfulfilled `dead_code` expectations. They did not fail clippy.

Failed attempts stay in place:

- `linux/focused/`: rustfmt exit 1, docker-run exit 1, elapsed 1 second. `rustfmt --check` on `lib.rs` walked into `physical_mapping.rs` because `skip_children` defaults to false. The diff is in the container console. The host file was restored and is not in `d80472d8`.
- `linux/focused-2/`: test exit 101, docker-run exit 101, elapsed 2 seconds. `usize::try_from` was matched as `Option`, and `?` was applied to `Option` inside `i64_of` while that function returned `Result`.
- `linux/focused-3/`: test exit 101, docker-run exit 101, elapsed 4 seconds. `CopyEncoder` had no `Debug`, so `unwrap_err` and `expect_err` in `copy_encoder_live.rs` did not compile.
- `linux/focused-4/`: test exit 0, clippy exit 101, docker-run exit 101, elapsed 5 seconds. `match_like_matches_macro` on `logical_matches` and `arrow_matches`.
- `linux/focused-5/`: rustfmt, test, and clippy exit 0. live exit 101, docker-run exit 101, elapsed 29 seconds. `postgres_decodes_copy_rows` panicked at `copy_encoder_live.rs:437`. PostgreSQL `flag::text` returned `true` and `false`. The assertion expected `t` and `f`. Every other cell matched, including the note with quote, tab, and newline, `NULL` versus a value, `i64::MIN`, bytea `00ff22090a`, amount `-123456.78` and `0.00`, dates `2000-01-01` and `1970-01-01`, and float8 hex `8000000000000000` and `3ff4000000000000`.

`linux/focused-6/` is the green proof. Exits: toolchain, snapshot, rustfmt-component, clippy-component, rustfmt, fetch, test, clippy, live, docker-run all 0. Elapsed 6 seconds. `overlay-notes.txt`: `lib_differs_from_fixed=1`, `mapping_unchanged=1`, `cargo_unchanged=1`, `type_contract_unchanged=1`. Package test: `copy_encoder_live` 5 passed, 1 ignored; the existing pipeline tests stayed green. Ignored test `postgres_decodes_copy_rows` then passed alone (`1 passed; 5 filtered out`). Postgres `ready=1`. `server_version` `17.10 (Debian 17.10-1.pgdg12+1)`, `server_version_num=170010`.

## Live result

`linux/focused-6/oracle.txt`:

```
count=2
md5=bfb64118b3d931be583473c4e5d942e9
local_md5=bfb64118b3d931be583473c4e5d942e9
chunks=4
```

Row 1 note is `a "quote"`, a tab, `and`, a newline, then `line`, with flag text `true`, `n` `-9223372036854775808`, raw hex `00ff22090a`, amount `-123456.78`, day `2000-01-01`, float8send `8000000000000000`. Row 2 note and raw are SQL NULL, flag text `false`, `n` `7`, amount `0.00`, day `1970-01-01`, float8send `3ff4000000000000`. The test writes that oracle before the hash assert. The same run asserted schema, values, the hash, and `assert_rolled_back`: `ROLLBACK` ready status `I`, and `to_regclass('"oo_copy_a145"."sample"')` null. Four chunks are header, one row, one row, trailer, each sent as its own `CopyData`.

## Cleanup

The focused-6 trap removed `oo-a145-eef831aa-cargo`, `oo-a145-eef831aa-target`, `oo-a145-eef831aa-pg6`, and network `oo-a145-eef831aa-net`, and removed both containers. A later check of label `oo.owner=a145`, name `oo-a145`, and session `eef831aa-dba4-4af2-a78b-541bc9e0a145` listed no containers, volumes, or networks. Nothing was pruned.

Independent review of `d80472d8a99afd66d8d0e71420f7be2bb62e65c8` is next. SP04-W02 was not checked. No push and no release.
