# ClickHouse pin

Observed by this session on the host that ran the container. The image was already local. This task did not retag it.

| Field | Value |
|---|---|
| Repository tag | `clickhouse/clickhouse-server:26.8.21.10` |
| Index digest | `sha256:d3fd302416d9523d5d64239d5713d1c16c20ecea2045ac70d7e941b376cb266e` |
| Image id used at runtime | `sha256:4ea38f8345ad67dc437b41638804421ee6d21e6d8c6f794bf4b28305541de2aa` |
| Platform | `linux/arm64` |
| Created | `2026-10-08T16:22:12Z` |
| `com.clickhouse.build.version` | `26.8.21.10` |
| `version()` over HTTP | `26.8.21.10` |
| `clickhouse-client --version` inside the running container | `ClickHouse client version 26.8.21.10 (official build).` |
| Host `clickhouse` binary | absent; preflight used the image client |

The same tag also publishes `linux/amd64` at `sha256:6cbe7dbc540d5a2107aabe1b1b717ed4cff87fba79f3918b7213b931d28f8941`. This machine ran the arm64 image directly.

Server license: Apache-2.0. The image's `/usr/share/doc/clickhouse-*` packages contain no copyright file. The upstream tag `v26.8.21.10-lts` `LICENSE` was saved during preflight (11389 bytes, sha256 `af5d33e7f3dbbd25e889d8bdc61c83562c257d20c22386bb04106cd22937341a`). It starts `Copyright 2016-2026 ClickHouse, Inc.` The tag `v26.8.21.10-stable` returned HTTP 404.

Driver pin, not compiled:

| Field | Value |
|---|---|
| Crate | `clickhouse` `0.15.2` |
| Role | official HTTP client, `RowBinaryWithNamesAndTypes` |
| License | `MIT OR Apache-2.0` |
| Checksum | `7654154fde4d97ec1321bfe4c3965570be6fcd09e091da8afc3166ab5ee27be2` |
| `rust-version` | `1.89.0` |
| Crate size | 337326 |
| Published | 2026-08-28 |
| Repository | `https://github.com/ClickHouse/clickhouse-rs` |
| Yanked | false |
| Default feature | `lz4` |

The crate floor 1.89 is older than the repo's pinned Linux image `docker.io/library/rust@sha256:0f24958fa693fe838859faf64a0ec78d382cd543a6b6efd0fa7a3bcb10ea90d5` (`rust:1.94.0`, `linux/arm64`, recorded in `testing/infra/images.pins`). That image was not started. A version comparison is not a compile.

Frozen base `577160c22de63adb9b38e0147f3bc2da893c4d9d` has no `clickhouse` entry in the root `Cargo.toml` or `Cargo.lock`.
