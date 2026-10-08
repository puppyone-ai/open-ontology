# Findings

Session `eef831aa-dba4-4af2-a78b-541bc9e0a393`. Limited to the feedback queue slice proved against commit `3be6264f18811b2ad88eedf986c342b15bdd0a16`.

## Frozen card

The contract `oo.feedback.receiver/v1` in that commit matches the original card.

1. Client-certificate URI is the project. `X-Project` is ignored. Missing certificate and a foreign CA close the handshake. A URI outside `schema/registry.md` is `403 forbidden`.
2. A body whose `Content-Length` is 32769 is `400 oversized` and leaves no receipt.
3. `feedback_receiver` can `INSERT` and `SELECT` in `oo_feedback_receiver` only. `UPDATE`, `DELETE`, `TRUNCATE`, and `pg_authid` are `42501`. It cannot `CONNECT` to `oo_audit`. `audit_ingest` cannot `CONNECT` to `oo_feedback`.
4. The same receipt and the same bytes replay the stored ack, including after the process is killed and started again. A different body is `409 conflict` and commits nothing.
5. An item project other than the certificate project is `403 cross_project`. A body project other than the certificate project is `403 forbidden`. Neither commits.
6. The prompt remains `feedback_item.item_text` and `source.feedback_text` on channel `untrusted_feedback`. The metric channel source does not contain it. The outcome SQL template stays `SELECT sum(amount) FROM logistics.shipment`.
7. `GET /v1/reviews` returns the certificate project's stored candidates and sources. `published`, `grants_issued`, and `roles_changed` are false. A published-true insert is `23514`. There is no `oo_r_reviewer` role. The process does not `GRANT` and does not publish.
8. `POST /v1/admin` and `POST /v1/reviews` are `405`. Unknown fields, duplicate keys, malformed JSON, empty items, and a C0 character are `400 protocol`. The unknown-field token and the C0 marker are not copied into the error body.

`selfbuild_suggestions::produce` is the candidate source. Seals on the stored source match `canonical` hashes of the operator documents. `knowledge_verified` stays false. Two identical gap items collapse to one stored feedback row (`dedup.feedback_collapsed` 1).

## Bounds

This run accepts the queue a review reader can query. It leaves the suggestion job, the review page, and the G gate after a person accepts a candidate. It leaves the audit collector, Gateway WAL, and Gateway routing of `report_gap`. It does not accept SP02-W06, SP13-W04, or a parent goal.

`GET /v1/reviews` count 6 is the shop candidate rows from the author live test plus this oracle on the same private database. `pg/receipt-counts.txt` is `shop=2` and `other=2` for the same reason. Each test stored one receipt per project.

`linux/proof/phases/clippy-version.txt` is the message from before `rustup component add clippy`. `raw/console.log` records `clippy 0.1.94` after that install. The component add is inside the builder. The root lock and the product lock were not regenerated.

`raw/recipe-sha256.txt` records the archive paths under `/tmp/oo-a393-...`. The digests are the reviewed file bytes. The secret directory is not in git.

The author report's launch head `c5913f893e36b5745f400af83e11894d7725b9c2` is earlier than the product commit. The reviewed bytes are the commit, not that earlier head.
