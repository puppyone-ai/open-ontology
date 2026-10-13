# DL01-W1-02 fixed admission review

Verdict: approved, bounded to the frozen child.

Source `fabc471609eedd313f7900009793ca153b61b187`. Evidence seal `bf617b5a90280a1826124f7882903e846eae35a2`. Dependency `577160c22de63adb9b38e0147f3bc2da893c4d9d`. Packet hashes: 652 of 652.

The missing budget unit on `oo.source.pgsql/v1` is a valid deny (`BUDGET_UNIT_UNKNOWN` at `source_report.budget_unit`). A fresh PostgreSQL 17.10 reader report was adapted with exit 2, identity preserved, and `admitted` false. Positive admission was an independent complete target report (`rev25kestrel`, unit `WallClock`), exit 0, `zero_write` basis `not_audited`. Live zero-write is `observe` exit 0, basis `pg_csvlog`, reader forbidden 0, with a separate DBA `INSERT` of `kestrel-marker-25`.

DBX-W02, the support matrix, and the 14-day nightly stay open.

Packet: `evidence/884ceef4-1d15-460f-a201-3d4c29d6a16f/DL01/admission-fixed-review/`.
