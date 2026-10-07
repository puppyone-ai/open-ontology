# Findings

## F1. One new grok-4.7 request produced a count statement and a clean query-pg receipt

The observer sat in front of the reviewed relay and saved the request and the completion. The request model is `grok-4.7`. The completion `model` field is `grok-4.7-build`. The relay did not rewrite the saved request. The content SQL and the proposal SQL are both `SELECT count(*) FROM depot.counter`. Approval was written by the reviewer after that proposal, with `automatic=false`. Execute spawned `query-pg`, exit 0, receipt clean, audit admit and complete, row `int` `3`, citation bound to `sess-a346` / `turn-a346`.

This is `APPROVE_LIMITED` for that trusted-operator chain only.

## F2. The SQL fingerprint is not the execution binding

`q_8bd1eadb5903e12d` is the fingerprint of `SELECT count(*) FROM depot.counter`. The live fixture's count case uses that same statement and session `sess-a300` / `turn-a300`. This receipt's proposal hash, session, citation receipt `b78ee58389eca7216fc7f6798404c2fc71fe5a528b83f3e11de7e410f08b5a20`, and `backend_start` `20261007175659726193` are the binding for this run.

The fixture north execution saved beside it is fingerprint `q_3782300da36af286`, the headcount statement, proposal `8b2bbc84952f23ff040bacbf4a6c7ed3f2886f8bd6a071e2ff2a82f0099e4a0b`, rows 1, 2, and 3. The a334 headcount receipt uses that same fingerprint and a different proposal, citation, and `backend_start`.

The shared tuple hash `bd23f5d409f27a9c20cda5376b95f2b6d2126b085ffbd2e52df96cf15d030c1c` is the role binding and pack `29ba57598c850e6321945a0a4335b7df1416178d40fd7f13c5c0d0f40ceddbe2`. It does not include the question or the SQL.

## F3. The a334 full completion was not in that packet, and one readiness hash label did not match the listing file

a334's readiness chain has the headcount proposal, a later approval, and a clean execute receipt. It does not retain the raw model request or the full completion. This review captured both through the observer.

The proof-3 per-file listing matches the relay tree at `8c825d89`, 21 of 21. The listing file sha256 is `e8fa1f56d315052210e3dc4e1897121960b346f7565940e3328a5f01a1eaed84`. A readiness note's `de2ab0ad2b5ae483880f44a9a37005e6f0f6523a87d5afc143a6fa96f629ca77` is not that file. The 28 relay tests were not rerun. `shasum` rechecked the a334 manifest: 593 of 593 OK. The stamp is `e79273e76678beb0aafa072eb097492a2daa498863571df5ebdb7013e3e5eb7e`.

## F4. Historical auth-store restoration stays historical

a334 `CLEANUP.txt` records `auth_files_restored=2`. a334 `CLEANUP-readiness.txt` records `auth_store_touched=false`. This review did not read, search, or edit an auth store. `CLEANUP.txt` in this packet records `auth_store_touched=false`.

## F5. Two failed attempts were kept, and neither sent the review chat

`linux/smoke/attempt-1` used a Docker tmpfs on `/tmp`. The approval tests spawn a shell there and returned `Fault` `io`. docker-run exit 101, test exit 101. The retry omitted the tmpfs, raised the pid limit to 4096, and omitted `rust-toolchain.toml` from the private archive copy. That smoke passed on rustc 1.94.0.

`host/attempt-listen-1` parsed `mode=600 bytes=6` with `awk` field splitting, so the mode check failed after OpenSSL exit 0. No client token was minted, no relay listened, and no chat was sent. The retry writes the mode and length on separate lines. Models GET then reported `model_available=true` and `selected_model_id=grok-4.7`.

## F6. The execute post-pass removed its own script

The final scan deletes a file that contains the marker `PRIVATE KEY`. `linux/execute/inside.sh` contains that marker, so the container removed it and wrote `inside.sh.redacted`. The script was restored from the text that ran. The execute was not repeated. The copied outputs scanned clean against both token files and both password files.

## F7. Host compile used rustc 1.98.1

The pinned Linux image cannot emit the Mach-O relay, and the upstream listener is `127.0.0.1:8317`. rustup 1.94 is absent. The host binaries are Mach-O arm64 from Homebrew rustc 1.98.1. Linux observer clippy and the dialogue execute used rustc 1.94.0. Locked dependencies were not updated.

## F8. The upstream token matches two reviewed sources as a substring

A byte search of the 6-byte upstream token file matches only `tools/sha256.rs` and `oracle/observe.rs`. The client token and both database passwords matched no file in this tree. The match was not printed. Those source files were kept as written. This tree has no token file, password file, or private key.
