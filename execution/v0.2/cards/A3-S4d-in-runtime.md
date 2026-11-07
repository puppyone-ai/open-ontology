## Card A3-S4d — lane A3: run `oo build` stages inside the lite runtime (D16)

TODO: S4-A-02 (feeds S4-M-02). Rebase `v0.2/a-chain` first (your S4c commits are integrated). Decision D16 (plan §9):
on lite/remote deployments every `oo build` stage (oo-load, Builder/pg-builder scripts, oo review compile inputs,
release-coordinator publish-retained, Gateway reload, token issuance) runs inside the deployed lite runtime image
(`deploy/lite/Dockerfile` already contains oo-load, pg-builder, selfbuild-budget, release-coordinator and the
pg-builder scripts), with stage configuration derived from stack.yaml plus deployment state under `work/operator`;
`--agent-target`/`--runtime-config` stay only as developer overrides. The npm package ships no helpers.
Lane M is finishing the offline bundle (`oo up --bundle`) in deploy/lite and CI — consume its launcher contract, don't
edit its files.
Commit after each step:
1. Deployment-derived stage configuration (lite local and `--target ssh`) and in-runtime execution (`docker exec` into
   the running worker/builder service or a one-shot `docker run` of the pinned runtime digest with the deployment's
   networks/secrets mounts); resume boundaries unchanged; tests.
2. Update lane H's probe expectations you found obsolete (CA prompt label, sidecar path) in `tools/oo-test/l1_clean.py`
   — test code only.
3. Live on this Mac with a fresh HOME and the npm-installed CLI only: `oo init` (TTY) → `oo up` → `oo build` → share
   prompt → prompt's own login/whoami/query, for PostgreSQL 17.11; then MySQL 8.4.7 and Oracle 23.5. Evidence per engine.
