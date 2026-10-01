# Test fixtures and Linux builder recipe

This directory is local test infrastructure. A green smoke log is not `./oo up`, not DEC-17, and not a production proof that a source is read-only. Fixture admin accounts exist so the test can create schema. It is not a P14, gVisor, Debezium, or SP00-W05 acceptance.

Image digests stay in `images.pins` and `images.lock.json`. Do not replace a digest with a floating tag. Compose uses `pull_policy: never`. A missing local image fails the run. It does not pull.

## Run identity

Every command needs `OO_RUN_ID` set to a fresh lowercase UUID. The evidence directory id is not a Docker run id, and these scripts do not default to one. `OO_RESUME=1` is the only way to continue a run whose root already exists. A second `up` of the same id without that flag exits 2 before Docker.

`OO_SECRET_DIR`, when set, must be exactly `/tmp/oo-env-$OO_RUN_ID/secrets`. Any other value exits 2 before a write or a Docker call. Secrets are eight files from `openssl rand -hex 24`: 48 lowercase hex digits, one trailing newline, mode 0600, owner the current user, one link, not a symlink.

The run root is `/tmp/oo-env-$OO_RUN_ID` (mode 0700). On macOS its real path may be `/private/tmp/oo-env-$OO_RUN_ID`. Anything else is refused.

## Commands

From the repo root:

```bash
export OO_RUN_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
testing/infra/builders/snapshot-inputs.sh "$PWD" "/tmp/oo-env-$OO_RUN_ID/snapshot"
testing/infra/builders/run-matrix.sh \
  "/tmp/oo-env-$OO_RUN_ID/snapshot" \
  "/tmp/oo-env-$OO_RUN_ID/logs/matrix"
```

`run-matrix.sh` runs four recipes, two at a time: Rust 1.98.1 and 1.94.0, each on `linux/arm64` and `linux/amd64`. Arm64 cargo jobs are 6. Emulated amd64 jobs are 2. Each container is capped at 8 GiB and 8 GiB swap. The container executes `testing/infra/builders/container-cargo.sh` from the read-only snapshot. It is not a live bind of the working tree.

`snapshot-inputs.sh` copies an explicit set. `crates/` is the seventeen workspace members. `contracts/schema`, `contracts/fixtures`, `contracts/evidence` (including `evidence-v1.json`, which `xtask` embeds), `examples/config`, and `examples/schema` are included. So are `ci/deny.toml`, `ci/gates.yaml`, `ci/baseline/gates-baseline.yaml`, `.github/workflows/nightly.yml`, `.github/workflows/pr.yml`, `.github/workflows/release.yml`, `supply-chain/harness-dependency-delta.json`, and the builder files the matrix and `xtask infra --check` read. The shipping locks are `Cargo.lock` and `bench/datagen/Cargo.lock`. The bench lock is there because `supply::tests::both_shipping_locks_clear_the_age_floor` reads that path. `bench/datagen/Cargo.toml` is not copied: that test does not open the manifest, and `supply --run-tools` is the only caller of `cargo-deny --manifest-path bench/datagen/Cargo.toml`. Copying the lock does not run the bench binary and does not prove the bench MSRV. A missing required path, a uid other than the runner, a symlink, or a control character in a path exits 2 before promotion. Evidence session trees, the rest of `bench/datagen` (sources, fixtures, and `target`), `docs`, `specs`, and `governance` are not copied.

`cargo fetch --locked` may use the network. `build`, `test`, and `clippy` then pass `--offline`. This is not an air-gap claim. The root filesystem stays writable because `rustup component add clippy` for 1.94.0 writes into the image. A download line after the offline build marker fails the wrapper even when the container exits 0. Test and clippy still run when an earlier phase fails. The wrapper exit is the last nonzero phase exit. Clippy uses `-D warnings` and an exit 101 is kept.

One recipe:

```bash
testing/infra/builders/cargo-locked.sh 1.98.1 linux/arm64 \
  "/tmp/oo-env-$OO_RUN_ID/snapshot" \
  "/tmp/oo-env-$OO_RUN_ID/logs/matrix/1.98.1-arm64"
```

Fixtures, each with its own fresh `OO_RUN_ID`:

```bash
testing/infra/compose/up.sh
testing/infra/compose/smoke.sh
testing/infra/compose/fault.sh prove
testing/infra/compose/cleanup.sh
```

`fault.sh prove` is also called by smoke. `fault.sh inject <listener> [cut|slow]` and `fault.sh clear <listener> [cut|slow]` only accept the six names in `compose/proxies.tsv`.

## What the fixture checks

Networks `control`, `agent`, `source`, `buffer`, and `state` are internal. One Toxiproxy container sits on each network. Listeners, with no credentials in the proxy, are:

- `pg-control` to `pg-control:5432`
- `pg-agent-primary` and `pg-agent-replica` to the Agent databases
- `mysql-source` to MySQL
- `valkey-buffer` and `valkey-state` on their own networks

Smoke inserts a fresh replay token, reads `pg_current_wal_lsn()` after the commit, and waits until the replica's replay LSN is at least that LSN and the row is visible while recovery is streaming. The reader then gets PostgreSQL `42501` on `oo_fixture.replay_probe`. A connection or schema error is not that denial. MySQL's reader gets `1142` for `INSERT` into `shipment_leg`, and that statement has to be in `mysql.general_log` with the log on and `log_output=TABLE`. `IDENTIFIED BY` must not appear there. Passwords are created while `general_log` is off. A secret in that log fails init and leaves the volume.

Valkey buffer uses `appendfsync always`. State uses `everysec`. Both must report `aof_enabled:1`, `aof_last_write_status:ok`, and a `valkey-server` uid other than 0. DNS must resolve on the service's own network and fail across networks. A missing `getent` fails both sides.

`fault.sh prove` runs a legal query through each listener, injects a timeout, requires the next query to fail, clears it, and requires the query to succeed. `valkey-buffer` also gets a 2 second latency toxic and the measured delay has to be at least 1.5 seconds. A wrong listener name exits 2 with `status=rejected_target` and does not call Docker. A toxic that does not show up in the API is `status=noop`, not success.

## Exit codes

`snapshot-inputs.sh`: 0 when two copies match the source and the snapshot is read-only. 1 when copies differ or hashing fails. Nothing is promoted, and the partial staging directory is removed. 2 for a bad run id, a dest outside the run root, a symlink, a credential-shaped name, a missing required file, a uid other than the runner, or a control character in a path.

`redact-log.sh`: 0 when every secret-dir entry was enumerated, the eight required files are valid, and the log has none of those values. Hidden names such as `.extra` and `..extra` are part of that scan. 1 when a value was present. Counts are printed before the bytes are replaced with `[REDACTED]`, and the exit stays 1. 2 when the log or secret dir is missing, a required file is invalid, an extra entry is not a valid secret, the enumeration is incomplete, or a name contains a newline or other control character. Raw secret values are not printed.

`cleanup.sh`: 2 for a bad run id, a secret-dir override, or a frozen tree whose seal does not match, before Docker and before deletion. 3 for an unexpected file, hidden name, symlink, hard link, or mode. 4 when a Docker label is not `oo.project=open-ontology`, `oo.owner=env`, this full run id, and scope `sp00-infra-fixture` or `sp00-infra-builder`. Deletion is only of those verified objects and the eight manifest files. `docker rm -f -v` on a verified container also drops anonymous volumes still attached to that container. Named volumes stay until their own label check passes. `..extra` and every other non-member name stops the run before `docker rm` and before the secret wipe.

`fault.sh`: 2 for an unknown listener. 1 for an API failure or a toxic that does not change the query. `prove` prints `status=restored` only after every owned listener recovers.

`debezium/pending.sh` exits 2 with `debezium_cdc=not_started`. The server is not started. P3 and P11 are not accepted.

`up.sh` exits 2 when the run root exists without `OO_RESUME=1`, when resume has no sealed tree, or when the secret set is partial. The first start copies this directory, writes `.run-id`, and seals every frozen file before the frozen scripts run. Resume and cleanup refuse a symlink, a missing seal, a changed executable, or an entry that is not in the seal, and they do that before Docker. A same-user process that can rewrite both the seal and the tree is outside this check. `dns-check.sh` deletes a probe only after the recorded container id matches the project, owner, full run id, fixture scope, and probe name, and the removal is `docker rm -f -v` of that id. The probe mounts a tmpfs on `/var/lib/postgresql/data` so the image volume does not allocate an anonymous volume. A pre-existing name is left in place. Compose config is scanned for the generated secrets, `docker.sock`, `/home/`, host networking, and privileged mode before `up`. A failed `docker logs` fails the secret scan.

## Cleanup

`cleanup.sh` lists containers, networks, and volumes by the full `oo.run_id` label, inspects each one, and refuses the run if any label does not match. It does not prune the daemon and it does not delete by a short prefix. Official image layers stay in the local store.

## Not done here

- `cargo audit`, `cargo deny`, license re-fetch, and native source checkouts
- 50m rows and F numbers
- Debezium offset, history, and transactional routing (P3, P11)
- gVisor. This engine's runtimes are `runc` and `io.containerd.runc.v2`. `runsc` is not installed. A runc result is not an AT-RT or P8 pass. Registering `runsc` would edit the host daemon, which these recipes do not do.

Command output for a run belongs in the evidence directory for that session. It proves that snapshot only. It does not accept SP00-W05. An independent review still has to read it.
