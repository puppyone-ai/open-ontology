# Retained MySQL fixture services

The historical complete QA cuts are 5.7 generation 3 / GTID 92 and 8.4
generation 6 / GTID 38. Subsequent real fixture I/U/D automatically reached
5.7 generation 4 / GTID 93 and 8.4 generation 7 / GTID 39 through native
capture, Loader, CAS/ACK, maintenance, formal publication and a new ready
Gateway. No model call was repeated for the original continuity test. Final new-Pack
5.7 QA subsequently verified generation 4; 8.4 original QA remains historical.
Independent full comparison and stable CLI results matched both live sources.

Run arbitrary supported CLI commands through the current ready cut:

```sh
python3 tools/mysql-preview/live-cli.py --runtime /Users/redux/.cache/oo-mysql-connection-whole/mysql57/live-cli-v1.json tables --json
python3 tools/mysql-preview/live-cli.py --runtime /Users/redux/.cache/oo-mysql-connection-whole/c1/resume-20261016/live-cli-v1.json query 'SELECT probe_id,badge FROM native_mysql_20261012.value_probe WHERE probe_id=1' --json
```

The alias selects only a published `gateway-ready.json` with successful
preflight. Normal serve calls existing maintenance/publication and creates the
next ready Gateway; the caller does not edit its Gateway address. All other
legal CLI arguments, including explicit authentication flags, pass through.

Current cut tokens expire at **2026-10-15 21:17:38.150 UTC** (5.7) and
**2026-10-15 21:17:35.827 UTC** (8.4). A running worker does not renew an
existing token. To renew through the existing official issuer, choose the
current ready Gateway container and write a distinct private token file:

```sh
docker exec oo-mysql57-semantics-g4 /fixed/d861fca92c8b527f8bc40cc688a0400350677c71315830fe48bf1a47a13297c3/issue-token issue /run-current/endpoint.json /run-current/token-admin.pass /run-current/ca.crt /run-current/grant.json /run-current/renewed-client-20261016.token
```

Then add `--token-file /run-current/renewed-client-20261016.token` to the alias.
For 8.4 use `oo-mysql8-semantics-g7`. Never overwrite the original QA token,
current token, WAL or receipts. Later cuts have their own private directory and
token, and their actual container is recorded in the selected ready record.

The retained services use the existing 86400-second native serve limit and
one-second schedule. The current committed-script services started at
2026-10-15 20:25:25 UTC and end by 2026-10-16 20:25:25 UTC. Their exact PIDs, argv hashes, starts, deadlines, latest
quiet records, current API processes and certificate dates are in the final
resource receipt. They are owned test processes, not an installed boot-time
service or automatic renewal system. Stop/restart at an ACK boundary using the
same private template with its current `after_generation`; run `whole.py
TEMPLATE NEW_PRIVATE_EVIDENCE_DIRECTORY serve`. Preserve the prior directory.

The target initially inherited only 20 connections. Concurrent sources exposed
real PostgreSQL slot exhaustion. It now has 100 connections and no CPU quota,
with the same data volume, system identifier, network IP, TLS and role fences.
The replaced container is retained stopped and must never start alongside the
new container on the same volume. The physical standby remains streaming.
Loop failure now retains native-state stderr/exit and terminates its exact
owned worker; only explicit SQLSTATE 53300 has three bounded attempts. Four
lifecycle tests include a real container child cleanup test. Failed original
live loops and connection-limit attempts remain in evidence.

Certificate expiry metadata is in `final-resources-v5.json`. The API server
certificate expires 2026-11-11 03:02:02 UTC; primary data TLS expires
2026-11-08 15:21:31 UTC; isolated 5.7 identity expires
2026-10-17 18:20:40 UTC. Credentials stay in private mounted files.

The installed default mysql recipe currently selects the complete 5.7 input.
The original 8.4 run/registration, model dialogues and WAL remain historical
proof. Root owns independent acceptance and any later shared-source release.

The current services run the immutable committed Python snapshot at
`/tmp/oo-mysql-connection-whole/sol-20261016/live-python-6cc2fe3f4/source`,
including whole.py, Follow and their Python dependencies. Its archive, per-file
hashes, actual argv and launch metadata are in `provenance.json`. The final
normal generation-4 recipe completed fresh maintenance, current
preflight and both random two-turn dialogues, then passed normal reentry and
49 pins. The official mysql whole registration refers to that actual run.
The currently running v6 private templates point to the new fixed API,
release API and coordinator. New source cuts flow through capture, Loader,
CAS/ACK, maintenance, publication and a new ready Gateway; the alias selects
the ready record. Existing bearer files still expire normally and require the
explicit distinct-file issuer command above.
