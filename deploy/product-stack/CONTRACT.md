# oo-product-stack

Local product startup for SP11-W06 / SP14-W02. This tree does not accept the parent or the goal.

Owned paths are `deploy/product-stack/**` and `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a395/SP14/product-stack/**`. Product crates, the root workspace, and `apps/web/deploy/**` are not edited.

## Profiles

| Profile | Runtime | Result on this host |
|---|---|---|
| dev | localprocess | may start fixed services; `production_signed` stays false |
| standard | runsc | fail closed when runsc is absent; runc is not a substitute |
| minimal | runsc, no physical replica | same fail closed gate |
| observability | pinned images only | refused; Grafana and other images are not pulled |

`other_arch` is the integration node. This stack does not take a second architecture builder.

## Fixed inputs

| Unit | Pin | Startup |
|---|---|---|
| control-identity | a6f92da8a0bf872f8a30efb116c2c7b14463505b | image only when the tree matches the pin |
| audit-ingest | 1078d69e8c3cf8dacb145198f1e4123c767a196b | image only when the tree matches the pin |
| PostgreSQL | 17.10 digest in RUNBOOK | one private database, 1 GiB, 1 CPU |
| session-control-api | 53017ed29, owner UUID a387 | localprocess only; this stack does not own that crate |
| selfbuild-control-api | 13ad15b4bc66979b96a2604c20e2ca4b20dd92f7 | image starts on the selfbuild profile; pack read still needs a management session and the five producer binaries |
| source-control-api | uncommitted, no lock | recipe only |
| release-control-api | clean committed tree recorded in build-selection.txt, path dep release-coordinator 9d5df65b6 | image and HTTPS when the tree is clean; active read still needs a management session |

Health is ready only when captured HTTP bodies satisfy the oracle. A listening line, a TCP connect, HTTP 401, or `dependency.not_ready` is not ready. The oracle rejects `production_signed`, `parent_accepted`, `goal_accepted`, or `runc_substituted` set to true on input.

Session `owner_uuid` in the operator document is the service pin `eef831aa-dba4-4af2-a78b-541bc9e0a387`. The stack UUID is a395. The session binary rejects any other owner.
