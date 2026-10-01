# Canonical logical value v1

Protocol id: `oo.canonical/v1`. Type policy id: `type-policy/v1`.

The hash is SHA-256 of one frame. The frame is logical: field names, type tags, and length-prefixed values. It is not an Arrow buffer, a driver memory image, or a JSON document. CDC and batch rows that share a logical schema and this type policy produce the same bytes. `type-policy/v1` does not trim, case-fold, or redact text. A later policy would be a different prefix and a different hash.

Integers and decimals are decimal strings. They are never IEEE-754 values and never JSON numbers. `+0` and `-0` become one non-negative zero. Decimal scale is part of the value: `1.20` (scale 2) is not `1.2` (scale 1). A coefficient longer than decimal128(38) stays exact: storage class `decimal256` through 76 digits, then `decimal_string`. Digits are not truncated.

## Frame

Multi-byte integers are big-endian. `u32len` is a `u32` byte count followed by that many bytes. Text is raw UTF-8 with no terminator inside the length.

Value frame:

```
magic          oo-canonical-value/v1 NUL          (22 bytes, not length-prefixed)
policy         u32len of UTF-8 "type-policy/v1"
field_count    u32
fields         field_count times: u32len name, type, u8 nullable
values         field_count times: presence, then a payload when presence is value
```

Primary-key frame uses magic `oo-canonical-pk/v1 NUL` (19 bytes). Each field omits the nullable byte. Every component must be a present value. NULL, missing, and unavailable are rejected. Components are the declared key columns in schema order. This crate does not decide that two database collations are the same string.

Hook digest is SHA-256 of `oo-hook-digest/v1 NUL`, then `u32` name count, then each name as `u32len`. An empty hook list is count 0. The digest field is the 32-byte hash, not a second hash of that hash.

`encode_value` returns the frame. Callers hash it with SHA-256. `encode_hook_digest` returns the SHA-256 bytes.

## Tags

Type tag (`u8`): 1 bool, 2 int, 3 decimal, 4 float64, 5 text, 6 bytes, 7 date, 8 time, 9 timestamp_naive, 10 timestamptz, 11 json, 12 array, 13 struct.

Decimal schema parameters: `u16` precision, `i16` scale. Precision 0 and scale `i16::MIN` (`-32768`) mean arbitrary numeric: each value carries its own scale. Any other precision 0 is rejected. A fixed decimal requires the value scale to equal the schema scale and the coefficient length to be at most the precision.

Array schema: tag, then the item type. Struct schema: tag, then a nested field list that includes nullable bits.

Presence (`u8`): 0 missing, 1 null, 2 value, 3 unavailable. Null requires the field to be nullable. Missing and unavailable do not. Text, bytes, SQL NULL, JSON null, and missing are different encodings.

| Payload | Bytes |
|---|---|
| bool | `u8` 0 or 1 |
| int | `u8` sign (0 non-negative, 1 negative), `u32len` ASCII digits, no leading zeros except `0` |
| decimal | `i16` scale, `u8` sign, `u32len` absolute coefficient |
| float64 | class `u8`: 1 every NaN, 2 both signed zeros, 3 +inf, 4 −inf, 5 finite then `u64` IEEE bits |
| text or bytes | `u32len` octets |
| date | `i32` year, `u8` month, `u8` day; impossible civil dates rejected |
| time | `i64` microseconds, MySQL TIME range |
| timestamp_naive | date fields, then `u8` hour, minute, second, `u32` micros |
| timestamptz | `i64` microseconds, second range −62135596800 through 253402300799 |
| json | one inner node |
| array | `u32` count, then each element as a field value (nullable item) |
| struct | each member as a field value, schema order |

JSON inner tags: 1 null, 2 bool, 3 number, 4 string, 5 array, 6 object. A number uses the decimal payload (`i16` scale, sign, `u32len` digits). The scale is signed. A negative scale means the coefficient is an integer number of powers of ten: scale −2 and coefficient `12` is the value 1200, not `1.00`. JSON numbers `1`, `1.0`, and `1e0` still normalize to the same non-negative coefficient and scale. Object keys are sorted by raw UTF-8 bytes. Duplicate keys are rejected. Arrays keep order and duplicate elements. A well-formed UTF-16 surrogate pair is one Unicode scalar. A lone or mismatched surrogate escape is rejected. The raw scalar and the escaped pair encode the same frame. JSON null is not SQL NULL.

## Budgets

The encoder rejects a frame that would pass a budget. It does not truncate.

| Budget | Limit |
|---|---|
| Canonical document | 16 MiB |
| One field value | 1 MiB |
| Field name | 256 bytes, non-empty |
| Coefficient digits | 8192 |
| Array elements | 100000 |
| Nesting | 32 |

## Independent vectors

These SHA-256 values were computed from the byte layout above with a separate SHA-256 (`/usr/bin/shasum -a 256` in the tests, and an independent hashlib check of the same layout). They are not produced by hashing the encoder output and then copying that result back.

| Vector | SHA-256 |
|---|---|
| Nullable bool `flag` = true | `436a78e4f7f6ed16a9f70330182c507b2510c8ce1a6615a0475b53e1ac5e6a92` |
| Same bool as a primary key | `4532b2281ae3da5a2fa0ed254a1973fb0eb05efe4e7ee0a0f87f3ce5b2945abc` |
| Nullable `decimal(10,2)` amount `1.20` | `901fc02862bb00cf6c16f01f55edac7d56bdfc6efeca2984feb5b2404a69266c` |
| Nullable JSON `{"b":1,"a":true}` after key sort | `4a2164bd50a64e5e179c23c54e26cb88e0878263499217f1f90181c63e88584a` |
| Empty hook list preimage | `20a03e6640fe64a910e8e706fa97ff8e74c5276400a2b90b5f87370cce22e774` |
| Hook name `mask_phone` | `5c6dfbe3d5bd88a79b2c8ad35014510cdd0160e88ca810a8771ad875e9085ab8` |

The bool value frame is magic, `u32len("type-policy/v1")`, field count 1, `u32len("flag")`, tag 1, nullable 1, presence 2, byte 1. The primary-key frame drops the nullable byte and changes the magic. The decimal frame adds schema `u16` 10, `i16` 2, nullable 1, then presence 2, scale 2, sign 0, digits `120`. The JSON frame is tag 11, nullable 1, presence 2, object tag 6, count 2, key `a` with bool 1, key `b` with number scale 0 digits `1`.

## What this protocol does not do

GB18030 and GBK decoding stay in SP03. Named IANA civil time is converted in `oo-core` with pinned `chrono-tz` 0.10.4 data (IANA tzdb 2025b). A gap, an overlap, an unknown zone, an impossible civil time, and a fractional second past microseconds fail closed. Overlap names both instants and does not pick an offset. `ZeroDatePolicy` applies only to a source zero date (`0000-00-00`), not to those failures. This is not a PostgreSQL or MySQL driver and it is not a second CDC path. This document does not define a PositionCodec. Database collation differences are not treated as equal. Arrow series-60 names used by batch metadata are hand-rolled display strings. `arrow-schema` was not linked, and its Display implementation was not executed.
