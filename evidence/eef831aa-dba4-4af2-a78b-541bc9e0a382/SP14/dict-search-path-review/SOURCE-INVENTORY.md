# Source inventory

Hashes are SHA-256 from `sha256sum` inside `oo-a382-proof` on the `git archive` of `9a7940ba5bae734b16928cd68913b0037c262c0f`. `proof/source-sha256.txt` is that output. The redactor keeps a 64-hex digest intact.

## Unchanged from be4b596481d83435df2ede8abd9ebb2a8647c706

| path | SHA-256 |
|---|---|
| `tools/calendar-store/migrations/oo_calendar_control_v1.sql` | `ec89628711eceb98228b754a4cf4db360427b2d9dcc9ef31ca41690de945a520` |
| `tools/enum-store/migrations/oo_enum_control_v1.sql` | `b4ef16188fa26d731493f9d7d3422661022aa18a47840c3ca0af8ee2122ad9dd` |
| `tools/calendar-store/Cargo.lock` | `a3c203e65af360f75cac875143a669e9ae91a73a02fa65d656db4d24db1cbe44` |
| `tools/enum-store/Cargo.lock` | `19619c1de9ad8fec8a11b62e4464d93c3789a5ecfdc519281b2f0bc45970c923` |

`git diff` of those four paths between the base and the fixed commit is empty. Lock shape at smoke time was `package_names=211 source_lines=208` for both.

## Reviewed at the fixed commit

| path | SHA-256 |
|---|---|
| `tools/calendar-store/src/adopt.rs` | `b131ddbc8154339946edf9a384813a652ea08167cee58e204c4951a86ee01783` |
| `tools/calendar-store/src/sql.rs` | `cd6bf94d288773c8d9f12e182d9458837d5b8cf4c0a6a9304f3fc1f4246656c9` |
| `tools/enum-store/src/adopt.rs` | `c44b4c5688c3783b254f46e00797673544f9ac9ee1849ec80b7a3dabeb826706` |
| `tools/enum-store/src/sql.rs` | `ce855040f6036184e1c740bb3fc6c6a0741530f4c3eff3fcf5788cc1d2763ce4` |
| `tools/calendar-store/tests/search_path.rs` | `d2e28a77ba30b77d4f6c4edd327c7a065fad450158a014b908f7279c098fdc16` |
| `tools/enum-store/tests/search_path.rs` | `215747110701305c3c91aa5a978e57eacbceefdae5102b7bf614a5e9b9987847` |

Tracked delta from the base to the fixed commit, `proof/product-delta-tracked.txt`: 6 files, 665 insertions, 12 deletions.

Path dependencies were taken from the same commit. Their tree ids are `proof/oo-core-tree.txt`, `proof/business-calendar-tree.txt`, and `proof/enum-history-tree.txt`.

The evidence walker `linux/hash-oracle/main.rs` is the a365 file, SHA-256 `e85c22eaa948586e8739d0976eea536d442408d950076a143bac15447ce404df`. `pg/pg-wrap.sh` is the a376 copy, SHA-256 `9ae5704c99af6f2d8c6b95ac220a7f8054ea62983f6028f1f75b0949f87e12b5`. Entrypoint flags are the reviewed ones named in `origin/recipes.txt`.
