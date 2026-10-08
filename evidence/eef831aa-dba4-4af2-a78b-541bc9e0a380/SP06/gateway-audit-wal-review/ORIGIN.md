# Provenance

Session `eef831aa-dba4-4af2-a78b-541bc9e0a380`. Independent review of frozen E55-D1/1. Product is fixed at `1078d69e8c3cf8dacb145198f1e4123c767a196b`. This note does not accept the parent, SP06, SP02-W06, public SQL, or a feedback receiver.

Reviewed recipes, read and not modified. Commit `10b78993641c88154c75c22219c79c1566e2cecd` (`deploy: parametrize trusted-demo resource identity`).

| Path | SHA-256 |
|---|---|
| `deploy/trusted-demo/pg/up.sh` | `904aed7ed10156576252da4be7ad828832aefe4628d15f67c1436f475cad9bff` |
| `deploy/trusted-demo/pg/pg-wrap.sh` | `05e7a4469b63096ddf444c4420410558dd251168cf1510c29779f08ec18ed42a` |
| `deploy/trusted-demo/linux/extract-libclang.sh` | `3a630d6f8ef3ccb29412114a568cf17879054ac7a8fd9551af46ecf69be3c4a5` |
| `deploy/trusted-demo/tools/sha256.rs` | `55779e18239912415f84413ac2bc3be34e71bbae10231db01ab5406af7b8a19e` |
| `deploy/trusted-demo/tools/select-debs.rs` | `54204d3c868f8bf55ab54a11c0929b6e57aa95c6229807a82b5dcc4025d85644` |

`pg-wrap.sh` is the process entrypoint, bind-mounted and invoked as `/fixture/pg-wrap.sh`. Its docker flags match `up.sh` `run_pg` (1GiB, 1 CPU, no published port, cap set from that function, `--pull never`). `up.sh` itself is not executed. Its manifest writes evidence under `SP11/trusted-demo-identity` and pins a different `fixed_source`. This review's evidence path is only `SP06/gateway-audit-wal-review`.

`extract-libclang.sh`, `sha256.rs`, and `select-debs.rs` are the files at those paths. The script's own cargo paths (`/cargo-home/...`) are used. `OO_DEMO_PHASE` is the only caller input the script reads.

Images, already present, not pulled:

- `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`
- `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`

Owned names: network `oo-a380-net`, postgres `oo-a380-pg`, builder `oo-a380-proof`, volumes `oo-a380-pgdata`, `oo-a380-cargo`, `oo-a380-target`. Secrets stay under `/tmp/oo-a380-eef831aa-dba4-4af2-a78b-541bc9e0a380` and are not committed.

The author packet `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a371/SP06/gateway-audit-wal/` was read and not modified.
