# Review recipe origins

Review session `eef831aa-dba4-4af2-a78b-541bc9e0a322`.
Product freeze `895864a5bf64e50cef84075d20fd7b0c9e41c264`.
CLI archive the product builds `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`.
No product file is edited. SHA-256 rows for these copies are `linux/preoverlay/recipes-sha256.txt` and `linux/preoverlay/deploy-sha256.txt`, written by `deploy/trusted-demo/tools/sha256_manifest.rs`.

| Copy | Origin |
| --- | --- |
| `recipes/a204-preoverlay-launch.sh` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` |
| `recipes/a200-proof-launch.sh` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a200/SP06/policy-catalog-review/linux/proof/launch.sh` |
| `recipes/a199-overlay-launch.sh` | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a199/SP05/delimited-reader-review/linux/overlay/launch.sh` |
| `source/deploy/trusted-demo/` | `git archive 895864a5 deploy/trusted-demo` |

The product launcher already records a204 overlay-3 `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee`, a199 overlay-3 `c837e9d18e1ae9fa9ae90d03f920d645f9b30d235a2384e630aa0a9c9782b69a`, and a200 proof `c57fcc9ee166c216b0d4786a2fefb1a0c3bab8dbedc9e2cba632100e516b7541` in `deploy/trusted-demo/ORIGIN.md`. This review runs that product launcher for the chain. It does not fork it.

`linux/preoverlay/launch.sh` starts from the a204 preoverlay launcher. The changes are the ones this review requires:

- Builder memory is 6 GiB and CPU is 1. The a204 file used 8 GiB and 4 CPUs. `CARGO_BUILD_JOBS` is 1.
- Global builder cap is 10.
- Names are `a322` and this run id. The preoverlay network and volumes are not the product names, and they are removed after a passing smoke.
- `/src` is a `git archive` of `25dfc902`. `/deploy` is the frozen demo tree. Nothing is copied into the archive.
- The host does not run `shasum`. The container runs `sha256_manifest`.
- The smoke stops before workspace binaries, PostgreSQL, and `apply-reviewed`.
