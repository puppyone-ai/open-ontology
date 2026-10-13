# Logistics demo

Public logistics source, empty local target, and a secret-free config template for the single-operator development loop.

The fixture is hubs 2000, shipments 8000, and tracking events 90000. Statuses are delivered, in_transit, and delayed. The generator is `testing/logistics-product-loop`. This directory does not contain passwords, tokens, or the oracle answers.

## Databases

`pg/up.sh` starts two pinned PostgreSQL 17 containers on `oo-a419-eef831aa-net`. Each has 1 GiB and 1 CPU. There is no host port.

| Role | Container | Database | Admin role |
| --- | --- | --- | --- |
| Source | `oo-a419-src-pg` | `oo_a419_src` | `oo_a419_src_admin` |
| Target | `oo-a419-dst-pg` | `oo_a419_dst` | `oo_a419_dst_admin` |

`pg/seed.sh` generates the COPY files and loads only the source. The reader role is `oo_a419_src_read`, with `default_transaction_read_only`, `statement_timeout` 30s, and `lock_timeout` 1s. Password files stay under `/tmp/oo-a419-eef831aa-dba4-4af2-a78b-541bc9e0a419/secrets/` at mode 0600.

`pg/verify.sh` reads the source once and compares it with `oracle.json`. That file is not a model prompt, a Knowledge Pack input, or a Pi file. The target must still have no `public.hubs` table.

Do not drop either database to hide a write. `close` keeps both volumes.

## Config

`config.example.json` is the operator-development shape. Replace `password_file`, `ca_file`, `bearer_file`, and `token_file` with private paths. Do not write secret bytes into the JSON. The model bearer file for this operator is `/tmp/oo-customer-demo-20261007/llm-token`. The endpoint is `http://127.0.0.1:8317/v1` and the model is `grok-4.7`.

Pi and `query-pg` paths are the published kit on volume `oo-a406-eef831aa-pi-kit`, mounted read-only at `/opt/pi-runtime-kit`. Pi is 1.0.4 and Node is 22.23.3. `catalog_bin` is filled with the built catalog binary at run time.

The product command, once the logistics question path is wired, is:

```text
minimal-product-loop run --config <private-config>
minimal-product-loop ask --config <private-config> --question <questions.json id or text>
minimal-product-loop close --config <private-config>
```

`questions.json` from `logistics-product-loop gold` has the six natural-language questions and no SQL. The acceptance card is:

```text
logistics-product-loop card --observation <measured.json> --out <dir>
```

Without a live observation the card exits 4 and leaves S9 and S10 `notdone`. A draft preview is not an approval or a publication.

## Measured 2026-10-09

`pg/up.sh`, `pg/seed.sh`, and `pg/verify.sh` exited 0. The source counts were 2000, 8000, and 90000, with the same distinct key counts. PostgreSQL returned the six oracle cells, including `6084.2588` and `9999999999999999999999999999999999.9999`. Hub badge hex values were empty, `00`, `ff`, and `0001ff`. The reader role reported `default_transaction_read_only=on`, `statement_timeout=30s`, and `lock_timeout=1s`. Its INSERT exited 1 and the hub count stayed 2000. The target had no `public.hubs`, `public.shipments`, or `public.tracking_events`. The system identifiers differed.

That measurement is the generated source load. It is not the product copy.

## What this directory does not claim

Copy, Self-build, Pi, the native gateway, and the six audited model answers belong to the product command. This recipe prepares the source and the empty target. It does not mark MP-01 through MP-06 passed. S9 and S10 stay notdone.
