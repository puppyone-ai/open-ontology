# Handoff

## Verdict

DL01-W1-07 is approved as a bounded claim-checker child on source `76928401921e941d0dc9454a0d2dd1db6088b928` and evidence seal `7216af5b0ee60f48e026449055f43dc84ca41242`. The caller-facing command remains:

```text
source-conformance check --matrix FILE [--historical-pg DIR] [--nightly FILE] [--package DIR]...
source-conformance map
```

Exit 0 means the report has no error findings. Exit 1 means a claim or evidence finding. Exit 2 is usage. `promoted` and `matrix_rewritten` stay false. The review binary was Linux arm64 debug `source-conformance`, sha256 `3d7b79c63e4b49b0bda5ac7039cfddc7490d348fda8902a9bea5a485e5eec88c`, size 4743216. It was built from the dependency archive plus the fixed crate and then removed with the private target volume. Raw probe files and argv are in `linux/probes/`.

## What a later consumer can rely on

The checker can refuse a bad matrix schema, a bad package schema, a raw hash mismatch, and a package whose target is absent from the matrix. It can accept one experimental cell when the mapped CC cases, raw hashes, raw exit 0, exact version, and cloud scope agree. Designed and candidate cells do not require suite packages. It will not promote a designed cell from a 14-day streak, will not treat a manual run as a suite day, and will recommend `experimental` after two trailing suite failures on a verified cell without rewriting the matrix. Managed variant `rds` cannot use container evidence for a runtime claim. The sealed historical PostgreSQL draft does not satisfy the CC suite.

Compile the same way: `git archive 577160c22de63adb9b38e0147f3bc2da893c4d9d`, overlay only `tools/source-conformance` from `76928401921e941d0dc9454a0d2dd1db6088b928`, and do not mount the live tree. The shared branch at the source commit contains `tools/source-support`; this child does not accept that workspace.

## What stays outside this approval

DBX-W14 remains open for engine-side suite execution and CI. Matrix integration remains pending. The current matrix still has zero verified and zero certified cells, and this review does not change that. `source_support::matrix::parse_support_matrix` remains the proposed API in `CONTRACT.md` until its immutable SHA exists. No production 14-day verified package was accepted.

## Resources

Removed at `2026-10-09T16:39:17Z`, each exit 0: container `oo-dl01w1-27-build`, network `oo-dl01w1-27-net`, volumes `oo-dl01w1-27-cargo` and `oo-dl01w1-27-target`, and `/tmp/oo-dl01w1-27`. No database was created. The retained image is `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`. Foreign builders were not stopped.

## Next step

Coordinator acceptance of this bounded child can be recorded without closing DBX-W14 or promoting a matrix grade. A source-support protocol SHA, nightly CI, and any verified cell are separate work.
