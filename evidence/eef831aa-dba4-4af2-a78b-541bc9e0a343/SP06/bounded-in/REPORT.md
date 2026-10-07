# REPORT

Session `eef831aa-dba4-4af2-a78b-541bc9e0a343`. Slice: bounded typed literal `IN` in the existing SQL guard.

Verdict: **APPROVE_LIMITED**. Parent SP06-W02, M1-I01, M1-I02, and the a317 goal stay open. This commit is not a new accepted item and does not change a denominator.

Product archive `21cc96241a841b47dbbcb116549030e38a643ba0`. Archive `sql_guard.rs` blob `1db8f3ed11a2d816d485bdd2865f37aff3e5cbed`. Head still has that blob. The worktree change is the overlay that the final builder compiled.

The guard admits `AEXPR_IN` when the left side is one visible scalar column and the right side is 1 to 32 integer, text, boolean, or `NULL` literals. The node stays `IN` or `NOT IN`. Values stay exact. Scope, parameter, rewrite, and budget checks stay. Unsupported shapes fail closed.

## Proof

Pre-edit smoke, overlay absent, Rust `1.94.0`, `oo-gateway` `sql_guard`: 24 passed, lock unchanged, container removed.

Final run, archive plus the two owned sources, one 6 GiB builder and one private PostgreSQL 17.10:

- `sql_guard`: 24 passed
- `bounded_in`: 11 passed, including `live_query_pg_executes_bounded_in`
- `cargo clippy -p oo-gateway --lib --test bounded_in --locked --offline -- -D warnings`: finished with no warning
- lock `b9aead241de2ef7e1703272b94e9e3828866393a1576b0d2b33050c1ddd4b474` unchanged

Live oracle:

```
qty_rows=3
null_in_rows=2
not_in_null_rows=0
not_in_rows=2
guard_refusals=7
rows_unchanged=yes
```

Executed shape from the guard, physical names, predicate unchanged:

```
SELECT pt_entry.pc_entry AS entry_id, pt_entry.pc_qty AS qty
FROM _oo.pt_entry
WHERE pt_entry.pc_entry IN (0, 1, 2)
ORDER BY pt_entry.pc_entry
```

Cells for that statement: `(0, int8 min)`, `(1, int8 max)`, `(2, 0)`.

## Resources

Images, already local: rust `sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5`, postgres `sha256:3aac91d19d5b7aff4c650dddfacc1a26c436459aafd4eb51ab0bf1748e990dbb`. Linux arm64. No pull. No prune. No host cargo during a builder. Final builder 6 GiB, 1 CPU, cap-drop ALL. PostgreSQL 1 GiB, 1 CPU, no published port, cap-drop ALL plus `CHOWN`, `DAC_OVERRIDE`, `FOWNER`, `SETUID`, `SETGID`. Secrets stayed under `/tmp/oo-a343-eef831aa-dba4-4af2-a78b-541bc9e0a343/secrets`.

a345's builder was left running. Cleanup removes only owner `a343` and this session.
