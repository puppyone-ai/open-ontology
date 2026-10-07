# Failed attempts

The guard source compiled on the first final run. Later failures were the new test, then a missing Clippy component. No container was left behind.

1. `linux/final/attempt-1`. `json!` rejected a method call inside the request macro. The column array is now built before the macro. Exit 101.

2. `linux/final/attempt-2`. `construct` took ownership of `GuardError` before the memo check. It now borrows. Exit 101.

3. `linux/final/attempt-3`. Assertions forbade the existing `AS entry_id` alias, and they expected `IN (-1)` to be a unary operator node. The parser stores `-1` as an integer constant. The tests and the slice contract now say that. The private fixture had already written `live/oracle.txt` with the expected counts. Exit 101.

4. `linux/final/attempt-4`. `sql_guard` 24 passed and `bounded_in` 11 passed. Clippy was not installed for toolchain 1.94.0. The next run installed it with `rustup component add clippy --toolchain 1.94.0`, the same step a200 used, then Clippy passed.

The accepting run is `linux/final` with `final.exit` 0.
