# Owned SSH regression fixture

Run sequentially (one source and one Agent DB at a time):

```sh
sh crates/oo-connectors/tests/ssh/prepare.sh
python3 crates/oo-connectors/tests/ssh/run.py --engine postgres --run-id ssh-pg-full
python3 crates/oo-connectors/tests/ssh/run.py --engine mysql --run-id ssh-mysql-full
```

Do not rebuild the binaries while a suite is running: the shared T1 receipt
records the binary hash at entry. `--scale`/`--tables` reductions are diagnostics,
not the full 2,000,000-row / 200-small-table acceptance.

The wrapper reuses the shared T1 seed, writer, correction, typed comparison and
T8 sampler unchanged. Its source is on a labelled internal network with no
published ports or host relay. Only the owned OpenSSH jump publishes a loopback
port. Database TLS still verifies `db.internal`, not the loopback listener.
Use `--subnet` if the default explicit test subnet overlaps an existing network;
the runner never removes someone else's network to create space.

Before T1, the live Rust test verifies discovery, mandatory non-TTY pinning,
fingerprint mismatch, wrong password, forwarding, same-listener reconnect and
shutdown. A PTY test rejects `no` and accepts only `yes`. PostgreSQL uses key
authentication; MySQL uses password authentication. Doctor uses a fixture-local
HOME and reports source/tunnel health; model health must remain blocked because
no model secret is supplied.

During the quiescent corrected load, a separate fixture-admin observer waits
for a PostgreSQL chunk FETCH and restarts only the owned jump. For MySQL,
a test-only opaque TLS relay in the jump holds an incomplete application-data
record after 1,234,567 server bytes, records the source's current statement,
then the observer kills/restarts the jump. It never decrypts database bytes.
This avoids missing MySQL's short-lived SELECTs through processlist polling.
Both cases require a nonzero loader exit and rerun the same target run ID.
The ordinary T1 comparison then verifies every table and replay must not add
target write bytes. This demonstrates safe restart through the existing
receipt/replan machinery, not transparent replay of a broken DB transaction,
source-real-time consistency, or skipping every previously read chunk.

The wrapper fails if either T1 or the unchanged T8 sampler fails. Proxy framing
has separate tests: `python3 crates/oo-connectors/tests/ssh/test_mysql_fault_proxy.py`.

All outputs are under `evidence/<run-id>`. Cleanup verifies both ownership
labels and removes only this run's containers, source volume, network, jump
image and generated secret directories. Failed attempts are not overwritten.
