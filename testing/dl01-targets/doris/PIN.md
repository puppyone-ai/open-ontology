# Apache Doris target pin

DL01-W1-12 target preparation. This is not a support-matrix promotion and not source-support acceptance.

| Item | Value |
|---|---|
| Product | Apache Doris |
| Version | 3.1.4 (matrix line 3.x; not 4.x, not a floating tag) |
| Topology | One FE + one BE, private bridge, one replica, no host ports |
| FE image | `docker.io/apache/doris:fe-3.1.4` |
| FE index | `sha256:e86abeb405d5d56ee60da7c3e132f0dddfdbe195206284231e55805c7fc3cf86` |
| FE linux/arm64 manifest | `sha256:bdff678e691b1950638d6ea5dc1aa14824a433602bd57bb6684f67133aacce25` |
| BE image | `docker.io/apache/doris:be-3.1.4` |
| BE index | `sha256:21cd6cdf39dfab5c08f21cc695c2d7541325273ee61ec0a8ab108b553e26925e` |
| BE linux/arm64 manifest | `sha256:013bea9345a905f088ce901b12c834ae908452fae2c0fbe0cc6adfee8ac75692` |
| Protocol | MySQL wire protocol on FE query port 9030 |
| Reader client | `docker.io/library/mysql@sha256:3f616910ccd923d5089d7889d491cefa65e24fcfa79d74db4084729f48c9cab1` (mysql 8.4.10 linux/arm64 from `testing/infra/images.pins`) |
| Client role | Protocol client only. The mysql server entrypoint is not used. |
| License | Apache-2.0. `LICENSE-dist.txt` sha256 `510188c1ab2c4efd7a834e532c2cc3196f58b92df7c0e728d8a15464a442d05c` (84943 bytes, same in the FE and BE images). `NOTICE.txt` sha256 `9abfb523ff2f9b8fcac9204ae478ef328efd2fb1531a270b504af5c99d9c4d0e` |
| Addresses | Bridge `192.168.208.0/24`. FE `192.168.208.10`. BE `192.168.208.20`. `172.31.12.0/24` overlaps an existing pool and was not created. The image entrypoint accepts only numeric addresses. |
| Memory | FE cgroup 1536m with heap overridden from `-Xmx8192m -Xms8192m` to 768m. BE cgroup 2304m, `mem_limit=1536M`, JVM heap overridden from `-Xmx2048m` to 256m. Client 64m. Two overlapping clients stay under 4GiB. |
| Not used | MySQL server, MariaDB server, StarRocks, amd64 emulation |

The amd64 manifests exist on the same tags and are not the runtime selected for this arm64 host. Default image heaps are inspected before FE/BE start and are not launched unchanged when they exceed the cap.
