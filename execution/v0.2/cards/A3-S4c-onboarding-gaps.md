## Card A3-S4c — lane A3: close the L1 clean-install blockers

TODO: S4-A-01, S4-A-02 (feeds S4-M-02). Rebase `v0.2/a-chain` first. Lane H's clean-install probe
(`tools/oo-test/l1_clean.py`, evidence `h-s4-l1-probe2-20261104`) found four product blockers on a fresh HOME with the
CLI installed from `npm pack`. Lane K's composite keys and lane M's lite cycle are integrated. Lane M is concurrently
building multi-arch images and an offline bundle (`deploy/lite`, CI) — coordinate through `oo up --bundle` rather than
editing its files. Fix, each with tests, commit after each:
1. `oo build --config stack.yaml` alone drives the chain: derive the agent target from stack.yaml (lite/remote) instead
   of requiring `--agent-target` (BUILD_AGENT_TARGET_REQUIRED, crates/oo-cli/src/admin/build.rs:134).
2. `oo up` after an npm install: the npm staging (`packages/cli/scripts/stage.mjs`) must ship the lite image digest
   sidecar the CLI reads (LITE_IMAGE_DIGEST_REQUIRED, crates/oo-cli/src/admin/lite.rs:24); when images are not local,
   `oo up` says exactly how to get them (`--bundle PATH` once lane M's bundle exists, or the pinned registry refs) instead
   of failing opaquely. Include dependency NOTICE files in the npm pack.
3. `oo init` TUI for MySQL/PostgreSQL TLS sources: add a step that imports the source CA from a file path (or chooses
   system trust) into the secret store, instead of requiring a pre-existing CA secret (wizard.rs:221). Also add the SSH
   jump-host step lane B specified (host/port/user, key or password secret_ref, out-of-band fingerprint confirmation,
   pin persisted; non-TTY requires host_key_sha256) using `oo_connectors::transport::ssh`.
4. The wizard's BUILD action runs the current `oo build` retained chain (wizard.rs:283), not the older draft flow, and
   the completion page prints the share prompt.
Then rerun lane H's probe yourself (`python tools/oo-test/l1_clean.py --run-id RUN --binary target/debug/oo`) on macOS
for PostgreSQL 17.11 to the share prompt and report what remains.
