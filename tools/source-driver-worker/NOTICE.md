source-driver-worker
Copyright 2026 the open-ontology authors

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    https://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.

This crate does not vendor Oracle, ODBC, JDBC, or JVM libraries.
arrow-array, arrow-buffer, arrow-ipc, and arrow-schema 60.0.0,
serde_json 1.0.151, and sha2 0.10.9 are fetched by Cargo at build time
under their own licenses. The isolate crate calls libc 0.2.190
`close_range` and `fcntl` only. The parent does not load a customer driver
artifact; it only hashes that file.

The v2 launcher in this crate talks to the Docker Engine socket on the
control plane. It does not link an Oracle client. The thin driver lives only
in the separate `oracle-child` workspace, which is not a member of this one.
That workspace carries its own NOTICE and SBOM.
