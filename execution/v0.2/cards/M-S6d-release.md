## Card M-S6d — lane M: release builds and the Quickstart dry run

TODO: S6-M-01, S6-M-02. Rebase on v0.2/product-loops first (keep your S5/S6 commits; resolve conflicts keeping both
sides). Disk is no longer a constraint. Lane A4 found: the Linux CLI built on the pinned Rust image needs glibc ≥ 2.39
(Debian 12 cannot run it).
1. Release CLI builds for linux-x64, linux-arm64 (glibc ≤ 2.31 via an older base or cargo-zigbuild, or musl static —
   choose the smaller change, verify by running `oo --version` and `oo doctor --help` on debian:11 and ubuntu:22.04
   containers), darwin-arm64, darwin-x64 (cross or CI-only — say which); wire them into packages/cli staging so
   `npm pack` contains every platform binary the npm package declares. CI workflow builds them; locally verify linux
   and darwin-arm64.
2. Quickstart dry run: in a fresh HOME install from the locally built `npm pack` tarball (the registry 0.2.0 is not
   published yet; say so in the doc as a temporary note only in your report, not in the doc), follow
   docs/guide/quickstart.md literally for PostgreSQL 17.11 to a working Agent query; fix every step that does not work
   as written (doc fix, or report the product gap with evidence).
3. Bundle tests (`test_bundle_dind.py`) green on the rebased tree.
