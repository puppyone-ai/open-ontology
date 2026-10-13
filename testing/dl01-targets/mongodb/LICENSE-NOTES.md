# License notes

Pinned image `mongo@sha256:d0d926f94df099bff534b7ee5b5986458131a22489dfff8664509af0c1e2ca9c`, MongoDB 8.0.32 Community, `linux/arm64`, size 305265587 bytes. Modules list is empty, so this is not an Enterprise build.

| Path inside the image | Observed license | sha256 |
|---|---|---|
| `/usr/share/doc/mongodb-org-server/copyright` | Server Side Public License (SSPL) | `f650d14ccf7865b7a4057254d4b2cc4d83b95258959f14acc03de97f7592f27e` |
| `/usr/share/doc/mongodb-mongosh/copyright` | Apache License 2.0 | `65da3b11b375b617c2c4c18fe20e970b399ab096d49a22092927df49cd84a868` |
| `/usr/lib/mongosh_crypt_v1.so` | MongoDB proprietary client-side field-level encryption library | `c8e1c0332d22bd40ea0a9f8019b640ffe09035eb82c5fcedcc5de86f79f80b99` |
| `/usr/bin/mongod` | binary digest only | `20dd9faca924b267843b3b80d662a4bf0ebe07de4b6462df6503e662af5de5fa` |
| `/usr/bin/mongosh` | binary digest only | `460f7d67ae7dc9ae9366db4d47cdbaee7a71a1d743a0c87ae58dbcd5ac7fbec7` |

The image's `mongodb-database-tools` package had no copyright file. This packet does not copy the SSPL text or `mongosh_crypt_v1.so` into Git. SSPL redistribution clearance is pending. The loopback fixture does not use TLS.
