# Captured PostgreSQL review fixtures

Synthetic `oo_a157` / `ledger_src` catalogs, not customer data. These are
byte-for-byte copies of the historical SP07 captured-pg-draft-review fixtures.
They live with the tests so a packaged checkout does not require the external
`evidence/eef831aa-dba4-4af2-a78b-541bc9e0a157/SP07/captured-pg-draft-review/probe/out`
tree. No historical evidence or expected hashes were rewritten.

- `catalog-before.json`: `de094222fcd5cb4e54c95a5ec8950c8fd2d55837bd1cff8cd3dd49673f8d56d9`
- `catalog-after.json`: `53e2fe7e68197f8698a2a9e2683dc2131426a0faea6bfb277594f9ba9ce52903`

Both CLI tests verify these hashes. Catalog comments and owner names are
intentional canaries for checking that draft output does not leak them.
