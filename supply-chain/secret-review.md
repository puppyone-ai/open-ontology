# Exact-content non-secret review

`secret-review.json` records an independent source review of scanner findings,
not an exception for production credentials or a release approval. Each entry
binds a repository-relative path, the entire file SHA-256, the detector kind,
and a reviewer rationale. A content change, relocation, new finding, malformed
review, missing hash, or missing rationale fails closed. No directory or file
extension is exempt. The older pending-classification list still fails.

The reviewed cases are runtime expressions, credential *file references*,
SQL/CLI syntax, detector strings, and deliberately synthetic local fixtures.
The three TLS fixture private keys are public, disposable loopback identities;
they must never be used by deployed services or packaged as runtime trust.
Their source locations are retained only for reproducible TLS behavior.

The scanner now reads large source/assets in overlapping bounded chunks instead
of refusing every file above 1 MiB. It also recognizes quoted JSON keys. Runtime expressions that resemble
credentials require exact-content review; punctuation is never an exemption. Known token
and URL detectors still run independently. Symlinks and unreadable files still
fail. CI builds the scanner under runner.temp so its own compiler output does
not contaminate the source scan; the scanner itself does not exclude target,
hidden, ignored, or new files.

The independent review was read-only and did not validate live credentials or
inspect deployed trust stores. Engineering source classification is not a
production security sign-off. Any later source changes require fresh review.
