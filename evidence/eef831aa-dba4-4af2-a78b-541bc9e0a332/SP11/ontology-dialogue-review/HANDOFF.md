# Handoff

Verdict **APPROVE_LIMITED** for `tools/ontology-dialogue` at tree `1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0` in commit `63566b60aabdf29a29bfe55c4866edf3c4081cfd`. Read `REPORT.md` for the fresh run and `FINDINGS.md` for the author manifest, the overlay, and the gateway binary. Parent RT-D05 stays open. This packet does not close SP11 and does not certify a release.

The pre-overlay container ran the nine committed tests and Clippy on the immutable archive before the live test. The live container then ran the committed `tests/live.rs` against PostgreSQL `oo-a332-pg`. `linux/live/oracle.txt` has `live_done=1`. `linux/live/north-execution.json` is the fresh execution. Its receipt hash differs from the author's saved receipt.

`ontology-dialogue propose` sends one question and one role Pack to one approved HTTPS completion. The model SQL stays `untrusted`. `ontology-dialogue execute` starts `query-pg --request` only after the approval names the current proposal hash, SQL hash, tuple hash, and Pack digest. The password file is read by the child. The approval file is a declaration. The audit receipt does not authenticate a session principal, and this command does not judge entailment.

The dialogue `Cargo.lock` stays the subset already accepted by `cargo fetch --locked`. Do not run `cargo update` or `cargo generate-lockfile` for this crate from this review. The root lock was not edited.

`MANIFEST.sha256` in this directory is produced after these reports and after `CLEANUP.txt`. `linux/manifest/launch.sh` compiles `tools/sha256_manifest.rs` in the pinned 1.94 image with a 6 GiB, 1 CPU, network-none builder. That run's console and hostconfig are `linux/manifest/console.log` and `linux/manifest/hostconfig.txt`. A second container rewrites `MANIFEST.sha256` after those files are closed. The tool skips its own output. The second stdout stays outside this tree so the count file is not a second mutable tail. `author-manifest/proof8-manifest.stdout` is the re-hash of the author's 439-entry inventory: 438 matches, one mismatch on `manifest-final.txt`.

Cleanup removes only a332 resources: container `oo-a332-pg`, network `oo-a332-eef831aa-net`, volumes `oo-a332-eef831aa-cargo`, `oo-a332-eef831aa-target`, and `oo-a332-eef831aa-pgdata`, and `/tmp/oo-a332-eef831aa-dba4-4af2-a78b-541bc9e0a332`. The result is `CLEANUP.txt`. The builders `oo-a332-preoverlay` and `oo-a332-live` were removed at the end of their launch scripts.
