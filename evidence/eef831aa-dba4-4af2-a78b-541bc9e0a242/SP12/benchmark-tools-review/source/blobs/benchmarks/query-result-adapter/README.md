# query-result-adapter

Standalone adapter from one `query-pg` JSONL stdout plus an explicit column binding to one typed actual result.

The result comparator accepts that object as `actual`. This crate does not see the expected side. The rules are `CONTRACT.md`.

```text
query-result-adapter < header-and-stdout.jsonl > actual.json
```

Exit 0 writes the typed result. Exit 2 refuses the stream. There is no match exit. Comparison is a separate command.
