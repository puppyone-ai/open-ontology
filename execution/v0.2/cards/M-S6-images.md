## Card M-S6 — lane M: multi-arch images, signing, lite offline bundle

TODO: S6-M-01 (exit: fresh install on both architectures). Works ahead; integration after the S5 exit. Rebase on
v0.2/product-loops first. Lane A3 edits crates/oo-cli/src/admin/** — do not; lane N edits tools/oo-test T3/T4 files.
1. One build definition for every lite runtime image (Gateway, worker/oo-load, Builder, scheduler, model-egress,
   operator bootstrap) producing linux/amd64 + linux/arm64 manifests with pinned base digests, SBOM and provenance;
   cosign keyless signing in CI (`.github/workflows/` or `ci/`; signing is CI-only — locally build unsigned and verify
   the pipeline with a local key from an untracked temp dir). No push to any registry from this machine.
2. `deploy/lite` offline bundle: `oo bundle create` (or a script) writes compose files, pinned image tarballs for one
   architecture, checksums and a manifest; `oo up --bundle PATH` installs without network (reuse lane A2's
   save/load verification code in deploy/lite/remote.py).
3. Exit: fresh install from the bundle on a clean linux/arm64 and a clean linux/amd64 Docker-in-Docker target (NOT
   privileged systemd containers — use docker:dind; never touch binfmt/sysctl), `oo status` healthy, then one scheduled
   cycle on PostgreSQL 17.11. Keep disk use bounded: delete tarballs and images you built when done.
