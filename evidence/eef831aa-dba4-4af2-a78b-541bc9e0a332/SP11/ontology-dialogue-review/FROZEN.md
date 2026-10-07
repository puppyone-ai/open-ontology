# Frozen review scope

UUID `eef831aa-dba4-4af2-a78b-541bc9e0a332`. This file is the review boundary. It was written before the private builder and before any review oracle was run.

The child under review is `tools/ontology-dialogue` at commit `63566b60aabdf29a29bfe55c4866edf3c4081cfd`, tree `1941b8a6a885ec85ac93da6cc6f4d45c8ecb48a0`. That tree is the same object as commit `e31635b213911c725159a23dbccc8463cc45cdd7`. No product file is edited. No new product test is added. The root lock is not edited.

The author packet is `evidence/eef831aa-dba4-4af2-a78b-541bc9e0a300/SP11/ontology-dialogue`. Its declared archive was `96afe70fd68a56ae8eec7504bf20e09ecd3cdf65`, which does not contain `tools/ontology-dialogue`. Proof-8 overlaid the tool onto that archive. This review does not rebuild that overlay. It archives `63566b60` and builds the committed tool from that commit.

Out of scope, and not accepted by a passing run: parent RT-D05, SP11 closure, hosted Gateway authentication, untrusted parser admission, `runsc`, an NL harness, and a release. The local approval file is an operator declaration. It is not authentication and it is not an entailment certificate.

The pre-overlay phase is the nine committed tests in `tests/propose.rs` and `tests/execute_approval.rs`, then Clippy. The live phase is the committed `tests/live.rs` against one new private PostgreSQL and one local TLS fixture. No further boundary is added unless that run fails.
