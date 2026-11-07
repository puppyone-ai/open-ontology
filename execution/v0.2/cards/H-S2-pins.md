## Card H-S2-pins — lane H: one patch version per P0 major, aligned with the T1 acceptance runs

TODO: S5-H-02 (pin consolidation part, pulled forward) and S0-H-04 (correction). Rebase on v0.2/product-loops first
(your S5 commits stay on your branch).
Problem: tools/oo-test's pins (testing/infra/images.pins + images.lock.json) and the engine lanes' T1 runs used different
patch versions, so evidence for one major is split across two builds (e.g. T1 on PostgreSQL 14.24, T2 on 14.23).
Coordinator decision: the P0 acceptance versions are the ones the T1/T8 acceptance ran on —
MySQL 5.7.44, 8.0.44, 8.4.7, 9.5.0; PostgreSQL 13.23, 14.24, 15.19, 16.15, 17.11, 18.6 (12.22 kept, experimental);
Oracle as pinned (gvenzl/oracle-free 23-slim = 23.5.0.24.07, oracle-xe 21-slim, oracle-xe 11).
Deliver: replace the MySQL 8.0.46/8.4.11/9.7.2 and PostgreSQL 14.23/15.18/16.14/17.10/18.4 entries in images.pins and
images.lock.json with the versions above (both platforms, `docker buildx imagetools inspect`, same record fields incl.
license/source/index digest), keep postgres-17.10 only if other non-v0.2 infra still references it (grep; say which), make
oo-test and t1.py/T2 read engine versions only from the pins (no tag literals elsewhere — grep and fix), update tests,
and run T0 on the aligned MySQL and PostgreSQL versions with evidence. One engine container at a time.
