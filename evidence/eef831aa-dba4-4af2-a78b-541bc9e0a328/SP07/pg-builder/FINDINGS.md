# Findings

`parent_acceptance` is false. Independent review has not been run.

## Proven

- Rust 1.94 focused-4: 10 `pg-builder` tests passed, 2 `pg-fixture` tests passed, clippy `-D warnings` clean, builder 6GiB/1CPU, not privileged, not OOM. `docker-run.exit` 0.
- The catalog container on the same image compiled the later seal fields and completed both passes. Every catalog, profile, comments, seal, cards, curated, and approve exit is 0.
- Private PostgreSQL 17.10 held 100000 synthetic rows. Grants after the model path match grants before.
- The loopback adapter opened `127.0.0.1:8317` for the real card set and recorded timeout at the frozen 30000 ms bound. Port 9 recorded `unavailable` without a peer. A 1 ms deadline recorded `timeout` at 2 ms.
- Failed generations exit 0 with `structural_draft`, `cache_store=not_stored`, and `repair=not_performed`. Review and pack then exit 1 with `review.blocked` and write no document.
- Curated bytes and shop grants were unchanged. Declared price stayed `unknown` with amount null. No invoice and no zero cost were written.
- The first live attempt is preserved. Its failure was a relative config path, `input.read`, and no socket.

## Not proven on the live endpoint

- A validated S5 table candidate and a validated field candidate were not produced. The success, truncated, and pass 2 calls all ended as deadline timeouts. The truncated config never received `finish_reason=length`.
- A durable cache hit was not produced. The second process used the same input hash, found no stored object, and opened a socket. `cache_hit_same_candidates=true` in `summary.txt` compares two timeout drafts.
- Live S9 approval and the S10 role pack were not applied. `classification.missing` was not reached because pack stopped on `review.blocked`.
- Field text absence from a pack was not observed live because no pack was compiled. The focused-4 test covers that path with a scripted transport.
- The 1.94 test suite was not re-run after `command_bytes` and `prepared_bytes` were added. The catalog container and the host binary compiled that source. Host rustc was 1.98.1.

## Collisions and secrets

The customer token file is 6 bytes. Evidence files do not contain it. `bearer_scan=clean` for the evidence tree. Source contains the same 6 bytes inside the hexadecimal alphabet constant and a synthetic numeric literal. That is a short-token collision, not a copied credential. Config files name `bearer_file` and do not embed the token.

## Seams

- Compiler seam `description-candidate.dataset_only`. Required extension: admit column candidates (`label`, `description`, `confidence`, `caveats`, `open_questions`) and apply them only through explicit review. Current description review writes dataset label, description, and grain. Description pack copies those three fields. This unit does not fake a pack and does not drop field candidates to claim acceptance.
- Numeric oid 1700 remains a query-executor seam for `shop.entry.amount`. Not owned here.
- Production provider HTTPS and 16-byte bearer rules were not weakened.
