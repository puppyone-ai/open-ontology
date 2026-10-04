# Handoff — a169

## Result

Blocking finding. The gateway crate is unchanged. Read `FINDINGS.md` before writing a policy compiler.

The pre-edit Linux 1.94 smoke of `oo-gateway` `--lib` passed, including the three architecture-driver tests named in `linux/inside.sh`. The private PostgreSQL 17.10 probe measured column grants and the `oo.claims` utility `SET` hole. Raw exits are next to the scripts. `SHA256SUMS` lists every other file in this directory. `SHA256SUMS.sha256` is the digest of that list.

## Do next

Admit a reviewed `tokio-postgres` stack in the root lockfile, or take an explicit architecture-gate change, in a slice that owns that lockfile or gate. Then implement column ACLs only. Reject every policy that contains a row filter. Drive the reviewed migration SQL through the fixture administrator. Prove grants on a shadow from `_oo.full_import_prepare_shadow`.

Leave these files alone while doing that: frozen migrations, source connector, pipeline library, core, builder, control, spec, and ledger. Do not grant as `oo_target_owner`. Do not put SQLx or an `oo-pipeline` dependency on `oo-gateway`.

## Review

Grok 4.7 in this session is the author, the tester, and the writer of these notes. That is one person. An independent review is still open. No release is claimed. No parent checkbox was changed.

## State left behind

Containers, volumes, network, and `/tmp` secrets for this session were removed (`cleanup.txt`). Failed install attempt 1 remains under `pg/attempts/1/`. The tree under test was `d9797edd63b976c1fb7ab823b9103ef347b192d4`, which contains fixed base `3dadf678ff0fbfdc1ddd2ac4144ee68cb2ba772c`.
