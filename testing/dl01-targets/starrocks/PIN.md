# StarRocks target pin

This file is the preparation pin for DL01-W1-13. It is not a support-matrix grade and it is not a MySQL or Apache Doris result.

| Field | Value |
|---|---|
| Product | StarRocks |
| Version tag | 3.5.21 |
| Image | `starrocks/allin1-ubuntu` |
| Platform | `linux/arm64` (native on this host; amd64 is not used) |
| Manifest digest | `sha256:9b98696ee0cb46831eabe197ae5e71f483ee3c2e16ab57fd27cb51a5c9ee6618` |
| Hub observation | 2026-10-09, tag `3.5.21` arm64 digest above. The same digest was also tagged `3.5-latest` that day. The run pulls the digest, not a floating tag. |
| Topology | One container, shared-nothing, one FE and one BE, `replication_num=1`. FE query port 9030 on the container network only. No host port is published. |
| Container name | `oo-dl01w1-13-starrocks` |
| Network | `oo-dl01w1-13` |
| Memory cap | `--memory=4g --memory-swap=4g` |
| CPU cap | `--cpus=1` |
| Driver route | MySQL wire protocol into the FE query port. The preparation client is the `mysql` client inside this image. That protocol does not make the product MySQL, MariaDB, or Doris. No Rust driver is compiled. |
| License | StarRocks upstream is Apache-2.0. The runtime records the license file actually present in the image, or records that the image has no license file. |
| Evidence class | `container` fixture. Not `verified`, not `certified`, not a hosted deployment. |

Identity gate, as observed on this digest: `current_version()` and `@@version_comment` are both `3.5.21-85fccf8`. They do not contain the word StarRocks. `@@version` is `8.0.33` and is the MySQL wire compatibility string, not the product version. The gate accepts that build-id shape only when `information_schema` also contains `be_tablets` and `tables_config`. A banner that contains MySQL or Doris is rejected. A build id without those tables is rejected.

Runtime image id, client version, license digest, and FE/BE membership are written by the journey into the evidence directory. If they differ from this pin, the evidence is the run and this file must be updated to the digest that actually ran.
