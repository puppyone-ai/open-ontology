# Handoff

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a395`. Card SP11-W06 / SP14-W02 local product startup.

Status: not complete. `evidence/.../SP14/product-stack/run/journey.json` ready is false. `parent_accepted` is false. `goal_accepted` is false. `production_signed` is false. Deployment on the run that started is `localprocess`.

## G1

G1 owns `apps/web/deploy/**`, including `README.md` and `APIBINDING.md`. This card did not edit those files and did not replace the G1 build output.

The fixed identity build stays commit `a6f92da8a0bf872f8a30efb116c2c7b14463505b`. Runtime origin stays unset. `oidc_unbound` stays as G1 recorded it. The same statement is in `deploy/product-stack/G1-HANDOFF.md`.

Frontend work stays with the original three frontend UUIDs. This card is not a frontend entry.

## What the next owner can use

Owned code is `deploy/product-stack/**`. Owned evidence is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a395/SP14/product-stack/**`. The runbook is `deploy/product-stack/RUNBOOK.md`. The contract is `deploy/product-stack/CONTRACT.md`.

Pins:

- Rust image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, pull never
- PostgreSQL `docker.io/library/postgres@sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`, one private database `oo_a395`, admin role `oo_fixture_admin`
- control-identity `a6f92da8a0bf872f8a30efb116c2c7b14463505b`
- audit-ingest `1078d69e8c3cf8dacb145198f1e4123c767a196b`
- session-control-api `53017ed29c98c49fb44a7eb18d5df6d5de273046`, service owner UUID `eef831aa-dba4-4af2-a78b-541bc9e0a387`
- source-control-api and release-control-api: recipe only, uncommitted, no lock, not compiled
- selfbuild-control-api: image built from `git archive HEAD`, compose profile left off

Builder recipe origin: `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a387/SP11/session-control-api/linux/impl/launch.sh`, SHA-256 `c522fe7bd898d640bbf9d9b4047d2bec74bb18c81e3c04fd7c75d1448bf8b88e`.

Local tags still present after cleanup, not pushed:

- `oo-a395-identity:local` `sha256:adef401ef8fc5d7d03c55c7a02e6c2b53311f1fa86cf645092c285c156a2680d`
- `oo-a395-audit:local` `sha256:29a1b858d4d83dc4ee1aebd72bda081e3a35eb54402eee04771f138b9826b2c7`
- `oo-a395-session:local` `sha256:0da76da3332ac39090140eeab9c5460de6f8cb4dceced451b2bf6122138f6eeb`
- `oo-a395-selfbuild:local` `sha256:1776de64ef45d3c0b48d3ac86dbbd38b5162ad838a29d18a04db9214128efbb2`

`run/init.exit` is 0. A new init refuses while that file remains in `run/`. Archive `run/` first if a later phase must start again. Do not start that phase to repaint this result.

## What is still required before ready

A later run needs real captured HTTP 200 bodies for all six exchanges, including a 100000-row source catalog, a selfbuild pack body with the producer binaries present, a release body with `production_ready` false and `certificate_class` `test_scope_not_production`, a session close receipt of 11 steps in phase `closed`, and an audit ack whose JSON matches `oo.audit.ingest/v1`. The oracle must be the one that reads those bodies. Invented JSON, TCP checks, and schema row counts do not pass.

On this Mac, standard and minimal stay refused until runsc exists. Do not substitute runc. Dev may come up only as localprocess with `production_signed` false.

B04 selfbuild 100k and the agent loop stay with that owner. This card must not attach to `oo-a373-pg`. E51 ordinary 100k was already verified elsewhere and was not reloaded here. PITR, identity sequence, and full state were not part of this run.

## Cleanup state at handoff

Owned containers, volumes, networks, and `/tmp/oo-a395-eef831aa-dba4-4af2-a78b-541bc9e0a395` are absent. `/tmp/oo-a395-oracle` is absent. Foreign names in `run/foreign-before.txt` were left in place. No docker prune. No registry push. No customer call.

Secrets, private keys, cookies, CSRF values, and the session grant bytes are not in this evidence tree. `run/session-grant.log` has the grant hash only.
