# Cold parse bench, 1000 files

Run `bench-resume1`, no LLM, `oo serve --runtime compose` (docs-db, docs-engine, doc-parser), local arm64 Docker Desktop.

- Wall time from Drive registration to every file final: **1425 s (23.8 min)**, target <= 30 min.
- Throughput 42.1 files/min over 3.7 MiB.
- Final states: `{"published": 1000}`.
- Mix generated: `{"csv": 59, "docx": 196, "md": 353, "pdf": 137, "pptx": 118, "txt": 59, "xlsx": 78}`.

| container | peak memory | limit |
|---|---|---|
| docs-engine | 66 MiB | 1.5 GiB |
| doc-parser | 1697 MiB | 3 GiB |
| docs-db | 411 MiB | 1 GiB |

`docker stats` samples: 8520 (about one per second).


## By format

| format | states |
|---|---|
| csv | `{"published": 59}` |
| docx | `{"published": 196}` |
| md | `{"published": 353}` |
| pdf | `{"published": 137}` |
| pptx | `{"published": 118}` |
| txt | `{"published": 59}` |
| xlsx | `{"published": 78}` |

## Checks

- [x] disk: at least 20 GiB free (84 GiB)
- [x] all files reached a final state (1000 files, {"published": 1000})
- [x] cold parse of the whole Drive within 30 minutes (23.8 min)
- [x] every file published (synthetic variants of golden files all parse) (1000/1000)
- [x] peak memory of docs-engine within 1.5 GiB (66 MiB)
- [x] peak memory of doc-parser within 3 GiB (1697 MiB)
- [x] peak memory of docs-db within 1 GiB (411 MiB)
- [x] cleanup left nothing (0 left)
