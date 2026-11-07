## Card A-S2f — lane A: verify the operator boundary, then seven healthy services and drafts

TODO: S2-A-02. Rebase on v0.2/product-loops first. Integrated since your last run: your D14/operator commits' base; the
Gateway operator restriction (82cc645d7: role lite_operator limited to GET /v1/health and /v1/meta, meta without a
publication, expiry by the token's canonical audience — adapted from your proposal); lane B's full token/CLI work; lane G's
Builder seams (catalog over oo-load views, client-certificate TLS) and its lite draft patch (00b7c6096).
First remove deploy/lite/operator-contract-request.patch from your tree (it is applied). Then (keep going; commit as you go):
1. `test_operator_boundary.py --gateway-boundary`: lite_operator gets 200 on /v1/health and /v1/meta, 403 on everything
   else incl. /v1/session/current; expiry values correct.
2. First `oo up` issues the operator token automatically; all seven services healthy in `oo status` after one scheduled
   no-op cycle.
3. One command from the PostgreSQL fixture to an unreviewed draft, then MySQL 8.4 and Oracle 23 (endpoint/key via the 0600
   secret files; bounded live usage). Record evidence per engine.
