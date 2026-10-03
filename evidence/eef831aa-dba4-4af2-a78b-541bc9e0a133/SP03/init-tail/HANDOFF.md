# Handoff

Source commit: `bd77b2520bcc3d63517bc49309002f25b58544b1`.

Parent: `6a6d2166ae6843a1d9bee043f1b3f4443fb30fbd`.

Fixed input: `79515fcf7e7032c8c212540eaae4b48a5957e3a2`.

Review the three files in that commit. The proof snapshot was the `79515fcf` archive plus that overlay. Do not treat a later Arrow `stream.rs` commit as part of this change. This session did not compile `stream.rs`.

Focused proof is green. Raw logs, exits, and oracles are under `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a133/SP03/init-tail/`. Failures are preserved in `proof/console-attempt1.log`, `proof/attempt2/`, `proof/attempt3/`, `proof/rest-console.log`, `proof/attempt4/`, `proof/attempt5/`, `proof/embed-console.log`, `proof/attempt6/`, and `proof/attempt7/`. The green TLS transcript is `proof/pg-console.log`. The reader SQL excerpt is `proof/pg-reader-sql.txt`. The independent row oracle is `proof/pg-oracle.txt`.

Qualifications to keep:

- Live view capacity is the sum of the two receive windows, not RSS.
- Caller-held frames and `fetch_all` rows are separate from that view.
- A dropped `init_mut` can charge one reserve (observed 16384) and then plateau.
- The TLS read fuse is 1 MiB view and 512 MiB RSS. The observed view was 8192.
- The 50000 row/s connector cap is not this `fetch_all` path.
- Embedded `sqlx-core` tests ran as a standalone package on bytes 1.12.1 because the workspace cannot `cargo test` that non-member.

The production bounded driver integration gate stays closed until an independent review of `bd77b252` accepts it. Do not open that gate from this handoff.

Own cleanup is recorded in `proof/cleanup.txt` after the reports. It removes this session's PostgreSQL container and secret directory. It does not prune Docker, and it does not touch another session's containers.
