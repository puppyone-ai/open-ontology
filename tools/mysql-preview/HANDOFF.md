# MySQL dual-version fixture handoff

The author delivery is complete; root independently decides acceptance and
later-source release. Actual MySQL 8.4 and additional required 5.7.44 both
completed their original product chains. The final current 5.7 six-field run
`a876396217504f558f8f2f9847f143de` also completed all six steps, current Pack
random two-by-two QA, strict gold/QID/original WAL verification, official
same-input reentry and 49 immutable pins. The current default registration is
5.7-specific, not automatic dual-version routing. Other engine rows are unchanged.

The final packet is
`/tmp/oo-mysql-connection-whole/sol-20261016/fixed-reference-complete-mysql-family-v2.json`.
It links the original 8.4 and 5.7 histories, current normal run, fixed Rust and
Python archives, exact ELFs, both new temporal-semantics Packs, original QID
WAL, full-table oracles, retained failures and actual live resource receipts.
Neither historical QA nor old failed manifests/tokens/WAL were rewritten.

Current source cuts are 5.7 generation 4 / GTID 93 and 8.4 generation 7 / GTID
39, reached through genuine post-QA fixture I/U/D and the formal serve path.
Both versions then consumed the new fixed API via fresh maintenance,
publication and read-only standby Gateway readiness without new DML or S5.
Both visible Packs explain authenticated zero-date-to-NULL conversion and
signed TIME-to-interval. All ten semantic CLI cases on each version match
independent source gold; denial, cancellation and rollback remain enforced.
The new 5.7 random QA actually covers the previously failing zero-date case.
Its product response model is authorized Grok 4.7; original S5 and historical
5.7 QA remain genuine Sol. The engineering implementation used native Sol medium.

Long services use immutable Python `6cc2fe3f4` plus new fixed API/release API/
coordinator from the exact 6042 base with only commit `911392259`'s two-file
semantic delta. They remain running with one-second healthy quiet cycles.
See [OPERATIONS.md](OPERATIONS.md) for callable stable CLI aliases, actual
one-hour token limits and official renewal, 86400-second service deadlines,
TLS expiry and safe same-volume target constraints. No boot daemon or bearer
autorenewal is claimed. The owned 5.7 server uses official amd64 emulation;
no performance or production/customer-owner certification is claimed.

See [REPORT.md](REPORT.md) for chain details. Original failures remain evidence,
including TLS, decoder, kill/recovery, SQL guard, lifecycle/capacity, provider
cooldown and the missing Pack mapping. No formal source count changes here.
