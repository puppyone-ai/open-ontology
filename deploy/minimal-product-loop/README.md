# Minimal product loop

One local operator. Development preview. Draft knowledge is not a publication.

Commands:

```
minimal-product-loop doctor --config /var/lib/oo/loop.json
minimal-product-loop run --config /var/lib/oo/loop.json
minimal-product-loop ask --config /var/lib/oo/loop.json --question count-entry
minimal-product-loop close --config /var/lib/oo/loop.json
```

`config.example.json` has paths only. Password, bearer, and gateway token stay in mode `0600` files. The example digest is a placeholder. Replace it with the sha256 of the pinned Pi and Node binaries before `ask`.

`knowledge` and `work` are sibling directories. The knowledge folder is the pack Pi may read. The work directory holds state, cache, and the Pi workspace. Do not place the Pi workspace under a mode `0700` parent that the Pi uid cannot traverse. The root broker keeps secret files at mode `0600` and its private directories at mode `0700`.

`Config::pi_uid` and `Config::pi_gid` return `65532` for this operator-development mode. `pi.pi_uid` and `pi.pi_gid` may be omitted. The only explicit value accepted for either field is the number `65532`. Zero and any other identity are rejected. There is no same-uid fallback. Production runsc is unchanged and is not selected by these fields.

`sealed_pack_input` checks the catalog, profile, validated generation, binding, and mapped tables before `write_sealed_pack`. The pack file writer owns that function. This crate does not substitute a handwritten pack. The input protocol is `oo.minimal-product-loop.pack-input/v1`. The receipt protocol is `oo.minimal-product-loop.pack-receipt/v1`.

The root broker that launches that Pi child is a separate runtime profile: `--cap-drop ALL --cap-add CHOWN --cap-add SETUID --cap-add SETGID --cap-add KILL --security-opt no-new-privileges:true`. It is not privileged and it does not use host network, a host socket, or a host home. Compiler and proof containers that do not launch Pi stay `--cap-drop ALL` with no capability added. The child effective, permitted, inheritable, and ambient sets must be empty and `NoNewPrivs` must be 1. A bounding set that still contains those four broker capabilities is not a failure. This profile does not claim runsc or containment of an escaped process.

PostgreSQL is the supported source type. Any other `database.type` returns unsupported from the `oo.source.pgsql/v1` decision and does not open a socket.

`run` invokes a sibling `mirror-invoke.json` when that file is present. A sidecar with no `mode` field is a cache replay: the manifest is already complete, the secret arguments are the absent sentinel, and the receipt provenance is `cache_replay`. A sidecar with `"mode": "fresh"` and no source config forwards private secret-file paths to one `copy` of an already admitted plan. A fresh sidecar that also names `source_config` and `admin_user` runs `discover` into a plan path that does not exist yet, then `copy` into a manifest and target config that do not exist yet. When that plan, manifest, and target already exist and the manifest is a complete fresh source seal, `run` does not discover again. It confirms with `copy`, which returns `replay_complete` before opening the source, and the receipt keeps `copy_origin` `fresh` with the sealed snapshot and import hash. Both fresh receipts use provenance `fresh_complete` and drop the secret paths before the receipt. `discover` is true only when this invocation ran discover. Secret bytes stay in the mode `0600` files. The product database section must name the host, port, database, and reader user written into that target config. A fresh source-config copy then grants `SELECT` on the admitted mapped columns to the existing `oo_r_devread` role and does not create a role. The configured tables must match the manifest. They are not limited to one fixture, and the frozen bulk import hash is only the acceptance label for the known public generation.

A sealed run reads the local Agent DB through the trusted TLS reader. It writes an `oo.source.pgsql/v1` catalog, a profile with `sample_policy` `none`, and comments from the mapped physical tables, and it restores the logical names from the sealed manifest. `_oo_pos` is excluded. Histograms and raw sample values stay withheld. The sibling provenance says `local_agent_db_capture` and `source_preflight_not_invoked`. Protocol serialization does not claim a catalog-pg execution. The existing compiler seals that document, assembles cards, and generates an unreviewed draft. A second generate must hit the local cache with the socket closed. `draft_ready` requires catalog status `validated`, self-build `generated_unreviewed`, and cache replay `hit`. A local catalog fault stays `blocked`, exits non-zero, and does not call the model.

Without the sidecar, `run` counts the configured PostgreSQL tables and calls the existing catalog-pg path. Shop gold row counts are not a runtime gate. A refused catalog, a missing catalog binary, or a model draft that is not validated writes status `blocked` and exits non-zero. `draft_ready` on that path means an unreviewed generation. The draft pack stays `unreviewed`. `publication` stays `not_published`. Production authentication and Git publication remain TODO.

A cache replay is the offline half. A fresh copy is a separate invocation of the same command and needs an authorized copy producer plus a source this operator can read. This command does not regrant a revoked source.

`ask` is one frozen acceptance question when the run recorded the shop or bulk acceptance set. Any other table list has no frozen question. Pi writes the statement. `oo query` posts to a local door that spawns the existing `query-pg` binary. `ask` refuses while status is `blocked` or self-build is not an unreviewed generation, and it does not start Pi. A missing Pi, `oo`, `query-pg`, or policy file exits 4 and does not execute a statement.

`close` revokes the local proxy session map. It stops a recorded Pi pid only when the command and start time still match. It does not drop the source database. The status is `released` only when the map and its marker are gone, the database is still observable, and no recorded Pi pid remains.

Shop question ids: `count-entry`, `fk-account`, `null-party`, `qty-bounds`, `amount-exact`, `null-display-count`.

Sealed bulk question ids: `count-batch-c`, `id-overlap`, `qty-above`, `null-amount`, `decimal-row`, `binary-row`. No foreign key is declared between the bulk tables. `binary-row` asks for a `bytea` value the gateway type set does not admit, so a match is not expected. `run` writes `work/dev-policy.json` for query-pg and `work/dev-policy-fact.json` to record that `auto_accepted` is not a human review.
