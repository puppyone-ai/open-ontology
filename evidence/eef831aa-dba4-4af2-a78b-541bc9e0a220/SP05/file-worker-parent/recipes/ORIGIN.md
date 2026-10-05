# Build recipe origin

Fixed archive `c2db9c430b822a824a1062b6c3c7345924df7dd8`.

| Source | Path | SHA-256 |
| --- | --- | --- |
| a204 preoverlay launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/launch.sh` | `08fa35c8c46b01c0817314c619675330f6bc46ecd8a147e7552770fab56e6841` |
| a204 preoverlay inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/preoverlay/inside.sh` | `b7ee6d634a279b61b49badc1f497fd1a181fdae49d4e21f0fe5f2dec4db87636` |
| a204 overlay-3 launch | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/launch.sh` | `78c8ef8c179c59317850c1b6a9ae53d3f883b8a1b24c2567fa0d3d26f1997eee` |
| a204 overlay-3 inside | `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a204/SP05/file-validation-cli-review/linux/overlay-3/inside.sh` | `a93a5e00832284b805ea89236d9aace547766080316962139d3514dc58f84928` |

Changes from those scripts: one container, 6 GiB, 1 CPU, `CARGO_BUILD_JOBS=1`, global builder cap 10, owner `a220`. The private extract is mounted read-write so the smoke can finish before the overlay is copied. The git object is not modified. No PostgreSQL. No live-repo bind.
