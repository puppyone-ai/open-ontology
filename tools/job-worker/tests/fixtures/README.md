# Native capture test fixture

`pin1-str_key.arrow` is a 936-byte synthetic Arrow IPC input used by the
`capture_source` test. It contains a string key and test rows only. The fixture
is checked in here so a clean checkout can compile and run the test without
depending on ignored local evidence. SHA-256:
`37d7ab015182b700c38c170b30000b7d80c02ec76600fe0ffe53f5e23ec5ee6c`.
