# Findings

These are limits of this child. They are not waivers for a later loader.

1. `adopt_target_generation` runs as its own statement on the dedicated connection before `BEGIN` of the write transaction. The generation update is durable even when the later transaction rolls back. Payload conflict (`OO010`) and the injected row failure therefore leave the adopted `target_generations` row in place. The live test shows the receipt and the user rows unchanged after those two refusals. It does not query `generation_count` again after them.

2. The installed `position_lineage` check is `^[a-z][a-z0-9_.:-]{0,127}$`. A 64-hex lineage is accepted only when it starts with `a` through `f`. The fixture lineage starts with `a`. `request_closed` rejects other hex-64 lineages before connect.

3. `db_cross_table_consistent` is true only when this process applied the bound tables in one local transaction and the stored receipt matches, including `cross_table_consistent` true. `source_cross_table_consistent` stays false. This child does not certify source or WAL cross-table order.

4. a299 spool approval is still pending. This proof does not accept the spool, the parent SP04 goal, or CDC-D04.

5. `_oo.schema_migrations` rejects `UPDATE` and `DELETE`. Drift was proved by inserting historical version `target-fence-receipts/2` with a checksum other than `59489035c1dba8d559efa9e2cc149f39a131495b247f803d49bf5a8a15ec3512` into a template copy of the installed database. The original catalog was left immutable. A missing catalog remains `capability_absent`, which the bare database covers.

6. sqlx `Display` for the stale receipt call is `target generation or owner is stale` and does not contain `OO013`. The test reads `DatabaseError::code()`.

7. `commit_apply_receipt` checks identity and segment shape before the fence. The stale probe uses a lineage and digests that pass those checks, so the server reaches `OO013`.

8. Over 200000 events, or any non-null segment, returns `needs_segments` before the password is read. This child is not the 500000-event cross-table spool.

9. The writer does not ACK, XACK, TRIM, or mirror a control database. `control_lease_valid` is recorded as consulted false and is not a fence.

10. Full-import migration `0005` is not required. Historical `0002` is accepted only when its stored checksum and phase match the frozen digest. The runtime must already hold installer-owned execute rights. This crate does not grant roles or take the advisory lock by name.
