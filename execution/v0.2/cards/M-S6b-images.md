## Card M-S6b — lane M: finish the bundle fresh-install exit

TODO: S6-M-01. Continue on `v0.2/m-pipeline` (your 9251a8439, b66253307 stay on the branch; rebase on
v0.2/product-loops first). Remaining from your report:
1. arm64: first-up operator provisioning and the migration helper must come from the bundle itself (no Cargo/repo on
   the target); then `oo status` healthy and one scheduled cycle on PostgreSQL 17.11 inside the isolated DinD.
2. amd64: nested DinD under Rosetta fails in a prestart hook (host limitation; plan D1 has no x86 host). Use an amd64
   *client* container with the host Docker socket mounted (sibling containers, `--platform linux/amd64` images) as the
   clean amd64 machine, and record that the daemon is the host's. Same exit as arm64.
3. CI signing: keep the workflow; record that OIDC keyless signing is verified only in CI (not run here).
Lane A3 is editing crates/oo-cli/src/admin/** and packages/cli staging (image sidecar, `oo up --bundle` guidance):
expose your bundle launcher through a function/flag contract and name it in the report; do not edit their files.
