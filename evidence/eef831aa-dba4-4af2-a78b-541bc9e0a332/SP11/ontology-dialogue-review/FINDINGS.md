# Findings

Independent review of the committed dialogue command. Parent RT-D05 stays open.

## F1. Author proof-8 inventory

The 439-entry `MANIFEST.sha256` has one mismatched path, `manifest-final.txt`. The sealed digest is the empty-file digest. The current file is the manifest tool's three-line stdout, beginning `files=439`. The other 438 entries match the current bytes. `MANIFEST.sha256` is absent from its own listing because the tool skips the output path. The proof-8 oracle, the phase exits, and `source.sha256` stay usable. The inventory is not wholly green.

## F2. Archive and overlay

Proof-8 built `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65` plus an overlay of `tools/ontology-dialogue`. That commit does not contain the tool. The committed tool at `63566b60` is tree `1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0`, and its 18 `source.sha256` rows match. This review built that commit. The dialogue lock's registry checksums are inside the description-provider lock, the root lock, and the answer-citations lock. The root lock object is the same at `96afe` and at `63566b60`.

## F3. Gateway binary

`crates/oo-gateway` at `63566b60` is not the `96afe` tree the author compiled. This run's `query-pg` is the fixed-commit binary. The north audit receipt SHA-256 is `3faa1cac8d0c8d74b74298abf23a7371ca8a11e459c7f046165f69719f220c05`. The author's saved receipt SHA-256 is `6ab9131dc3b01a6c26f8d6f8ba20dbac6b7b9b114dd1e0b8f5d013ed4ea62fdd`. The admitted `query_id` is `q_3782300da36af286` in both runs. The proposal hash and the projection SQL hash match because those inputs are the fixture. `crates/oo-pipeline` is the same tree, so the migration files the live test applied are the same text. `crates/oo-control` differs and was not a separate review target. The numeric gateway change is inside the child binary this command spawned. This review does not approve that gateway change as its own unit.

## F4. HTTP failure marker

`prove_http_failure` expects `Fault::provider` from `propose`, then writes a stand-in `query-pg` and asserts that its marker file is absent. It never calls `execute`. `http_execute_called=false` is written after that assertion. `propose.rs` has no process spawn, and `auto_execute` is false. The provider fault is real. The marker is not a spawn refusal.

## F5. Persisted artifacts

The north execution and the model request are saved. The count execution and the truncation execution are not. The frozen test asserts the count cell `3` and disclosure `querytruncated` before it records `count_receipt=clean` and `truncated_disclosure=querytruncated`. Those two oracle lines are the surviving record of those asserts.

## F6. Libclang message

`linux/live/console.log` contains `ls: cannot access '/cargo-home/a332-clang-root/usr/lib/aarch64-linux-gnu/libclang.so*'`. `libclang-ldd.txt` resolves `libLLVM.so.19.1` from that private root. `libclang-load.exit` is 0. `query-pg` then built.

## F7. Bounds that held

One HTTPS POST, redirect policy none, retry never, no proxy, HTTP/1 only, and the fixture CA. The model body scanned clean of the bearer and of `password=`. Stale Pack and tampered SQL returned `approval` before a child, per the frozen test and `execute.rs`, which hashes the binary only after those checks. Write SQL and the south role returned child exit 6 with receipt `absent`. `pg-onboarding --help` exited 2. Grants came from `install-column-acl` and `provision-mapping-reader`. Fifteen Rust files were hashed. `execute_trusted_select` is absent. `#![forbid(unsafe_code)]` is set on the library and the binary.

## F8. Left to parent work

Hosted Gateway authentication, untrusted parser admission, `runsc`, and the NL harness are outside this command. The local approval is not authenticated authentication and it is not an entailment certificate. A local-LLM path owned by another session is not this transport.
