# Review 3 — bounded composition

Scope: one int8 quantity column through import, column ACL, mapping provision, analyze, and query. Policy generation is mapping `run_generation`. The target fence stays distinct. No builder for this note.

## Checks

`live/generations.txt` records source generation 48, mapping run generation 48, target generation 1, fence generation 1. `live/outcome.json` records policy generation 48 and `policy_generation_is_fence` false. Import run id `7c8ed96f-4c75-4a25-86eb-e3a49d0d4ed0`.

`live/reviewed-policy.stdout` generation is 48. `amount` is confirmed quantity with pii none. Digest `9675eb873e3222991d0759e1ac06460957ce144ea0ea2ad610cd139d03e2a8e6` matches the mapping digest.

Step exits: `import-pg-not-ready` 1, `init-pg` 0, `import-pg` 0, `install-column-acl` 0, `provision-mapping-reader` 0, `analyze-pg` 0, `query-ticket` 0. Analyze receipt source generation is 48, `published` false, `estimate_is_exact` false.

`live/steps/query-ticket.stdout` rows:

| ticket_id | amount |
|---|---|
| 1 | -9223372036854775808 |
| 2 | 9223372036854775807 |
| 3 | 0 |

The summary is `succeeded`, `returned_rows` 3, `end_audit_ok` true. TLS server version is PostgreSQL 17.10 and `ssl=true`.

This chain does not repeat peek, customer, hidden, insert, star, or role-pack. Int2 and int4 were not queried live.

## Verdict

APPROVE_LIMITED. The live policy generation is the stored mapping run generation. The adopted fence stayed 1. The int8 extremes came back unchanged. Forcing the fence onto the policy would fail this harness.
