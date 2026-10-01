# Encoding fixtures

These two files are the byte oracle for the legacy-encoding smell. The generator writes the same bytes into `source-inputs/encoding/` when the smell is on. The hex form lives in UTF-8 metadata. The bytes themselves are not placed inside JSON.

| File | Encoding | Bytes | Meaning |
| --- | --- | --- | --- |
| `gbk-huanxiong.bin` | GBK | `e4 bd d0 dc ca d0 be af b2 ec be d6` | 浣熊市警察局 |
| `gb18030-rare.bin` | GB18030 | `95 32 82 36` | U+20000 |

Both sequences are invalid UTF-8. `tests/contract.rs` compares a generated xs dataset to these files.

Coverage of the other bad-smell classes is in [coverage.md](coverage.md).
