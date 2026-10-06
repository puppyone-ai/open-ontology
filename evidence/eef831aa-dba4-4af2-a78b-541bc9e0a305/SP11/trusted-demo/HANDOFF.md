# Handoff

Owned paths:

- `deploy/trusted-demo/**`
- `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a305/SP11/trusted-demo/**`

No product path was edited. The binaries were built from `git archive` of `25dfc9022780c3deff12f37b7ad7ea4ed1fb6480`. A copy of `tools/pg-onboarding` from that archive is `source/tools/pg-onboarding/`. `source/ARCHIVE-COMMIT` is the commit. a207 remains the only writer of the connector manifest and the root lock. HEAD at report time was `4d9cc7529b4968f7c7c99dfafaaea3ec4cd6eac1`, and the fixed commit is an ancestor. Do not treat a later HEAD as a different archive for this demo.

This directory does not accept SP11-W06, M1, I01, or I02. It is not a release, not authentication, not runsc, and not a shared multitenant deployment. The same session wrote and reviewed it. That review is not an independent approval.

## What the next reader should open

- Contract: `deploy/trusted-demo/CONTRACT.md`. Source id is `public_shop` after attempt 2.
- Chinese operator note: `deploy/trusted-demo/README.md`.
- Recipe origins and hashes: `deploy/trusted-demo/ORIGIN.md`.
- Passing command: `linux/public-attempt-3/` and `linux/smoke/`. `docker-run.exit` is 0 in both. `live/receipt.json` status is `ready_for_trusted_query`.
- Failed attempts, kept: `linux/public/`, `linux/public-attempt-1/`, `linux/public-attempt-2/failed-run/`.
- Operator stage: `live/work/operator-stage.txt`.
- Bad review that stopped before ACL: `live/bad-run/`.
- Secret scans: `secret-scan.txt`, `smoke-secret-scan.txt`.
- Binary hashes: `linux/public-attempt-3/bin-sha256.txt`.
- In-container file walks: `linux/public-attempt-3/manifest.sha256`, `linux/smoke/manifest.sha256`.
- Later host inventory: `SHA256SUMS`. It is not a member of its own list.

The in-container walker skips its output path and runs before the host finishes the container log. These files are absent from the in-container walk, or the walk's copy of `console.log` is not the final host copy:

- `linux/public-attempt-3/hostconfig.txt`
- `linux/public-attempt-3/mounts.txt`
- `linux/public-attempt-3/tmpfs.txt`
- `linux/public-attempt-3/invocation.txt`
- `linux/public-attempt-3/docker-run.exit`
- `linux/public-attempt-3/container-removed.txt`
- `linux/public-attempt-3/console.log`
- `linux/public-attempt-3/host-stdout.txt`
- `linux/public-attempt-3/host-stderr.txt`
- `linux/public-attempt-3/host.exit`
- the same host-written names under `linux/smoke/`
- `start-public.stdout`, `start-public.stderr`, `start-public.exit`, `start-public.state`
- `REPORT.md`, `FINDINGS.md`, `HANDOFF.md`, `SHA256SUMS`

## Commands

Success entry, from the repository root:

```bash
./deploy/trusted-demo/start.sh --public-fixture
```

That command refuses to overwrite an existing `live/run`. After a ready receipt, repeat the query with:

```bash
./deploy/trusted-demo/start.sh --smoke
```

`--smoke` keeps the SQL, the role, and the policy id, and writes a new audit file. The receipt's `query-pg --request` path is the command that already ran. Running it again as-is fails because that audit file exists.

## Still running

`oo-a305-pg` is up. Id `da567a83607612931de313d0dc68ce6f0b3382e041f2f08f098e531a6bed003a`. Volume `oo-a305-eef831aa-pgdata`, cargo volume `oo-a305-eef831aa-cargo`, target volume `oo-a305-eef831aa-target`, network `oo-a305-eef831aa-net`, and `/tmp/oo-a305-eef831aa-dba4-4af2-a78b-541bc9e0a305` are still present. Builders `oo-a305-build` and `oo-a305-smoke` were removed after they exited. Passwords and the CA private key are only under that private tmp directory.

Stop containers for this UUID and leave volumes, secrets, and evidence:

```bash
./deploy/trusted-demo/start.sh --stop
```

Remove this UUID's containers, labeled volumes, labeled network, and private tmp directory. This does not prune and does not touch another owner:

```bash
./deploy/trusted-demo/start.sh --clean
```

## Outside this demo

The ops console, Compose parent, runsc, IdP, natural-language surface, and the arm64/amd64 matrix remain later work. A new expansion needs its own frozen contract before code.
