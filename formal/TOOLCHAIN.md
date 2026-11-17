# Formal verification toolchain (WS-FV, card V-S1-01)

Pinned for spec 80 section 6. All tools below are **development and CI tools**.
None is linked into, copied into, or shipped with any OO or Puppyone artifact
(binary, image, Compose profile, offline bundle), so none is a "default
dependency" under `supply-chain/policy.md` and 90 section 2; the admission
records in `supply-chain/` (`formal-toolchain-audit.json`) are filed for the
CI supply chain only. Models, traces and scripts under `formal/` are source
files, not runtime code.

Recorded on 2026-11-17, macOS arm64 (Darwin 27.2.0), 1.8 TB disk, 165 GB free.

| Tool | Pinned version | Source | Published | License (SPDX) | Digest |
|---|---|---|---|---|---|
| Quint | 0.33.1 | npm `@informalsystems/quint` (repo `quint-co/quint`) | 2026-10-28T10:38:19Z | Apache-2.0 | npm integrity `sha512-EYsgxr9SEsumYiix6pD8jbXPiPJoNbQaUxnrLE+dSVL+B9eQHPkVjSW7CBFEoBeMhau/1n70DAQOKvgxa1pVfw==`, tarball sha1 `d70fb010e3e2927b0d9daab2a1ac14be83ce7851` |
| Quint Rust evaluator (backend of `quint run`) | v0.7.1 | downloaded by Quint 0.33.1 on first `quint run` into `~/.quint/rust-evaluator-v0.7.1/` | n/a | Apache-2.0 | `quint_evaluator` sha256 `c918492321c1f1db0be02477e4520c4c29ecb42afeaaaefed87b36f16fc21dd8` (macOS arm64) |
| Apalache | 0.62.4 | GitHub release `apalache-mc/apalache` v0.62.4, fetched by `quint verify` (default for Quint 0.33.1) into `~/.quint/apalache-dist-0.62.4/` | 2026-10-12T10:00:59Z | Apache-2.0 | release `apalache-0.62.4.tgz` sha256 `b781b53bba32fac14ac2e02b0332e1dc2edc64cb7b850c0608d8142cc7aa2ef4` (from the release's `sha256sum.txt`); installed `lib/apalache.jar` sha256 `e5223915507b65cc5f1638bd037ede60f8a4fc1644aff46355779f9b05df9262` |
| Kani (`kani-verifier`) | 0.68.0 | crates.io | 2026-09-16T03:50:19Z | MIT OR Apache-2.0 (bundle also carries CBMC 6.11.0, BSD-4-Clause, and BSD/MIT parts, see `~/.kani/kani-0.68.0/license-notes.txt`) | crate sha256 `842fabd2ba8b13babab862e21fef2946b581d493f23062d5ce86266630f1d1ae` |
| Kani's Rust toolchain | `nightly-2026-08-21` (rustc 1.100.0-nightly, 8925ea358) | installed by `cargo kani setup` via rustup, recorded in `~/.kani/kani-0.68.0/rust-toolchain-version` | n/a | MIT OR Apache-2.0 | managed by rustup |
| OpenJDK (Apalache runtime, needs 17+) | 21.0.13 (Homebrew `openjdk@21`, keg-only) | Homebrew | 2026-10-24 (build date) | GPL-2.0 WITH Classpath-Exception-2.0 | Homebrew bottle |
| rustup (Kani setup only) | 1.29.3 (Homebrew `rustup`, keg-only) | Homebrew | 2026-11-02 | MIT OR Apache-2.0 | Homebrew bottle |
| quint-connect (Rust crate) | 0.1.2 (candidate) | crates.io | | | **not used**. The V-S2-01 conformance test parses ITF JSON with `serde_json` (already a direct dependency of `oo-docs`), so no `Cargo.toml`/`Cargo.lock` change and no admission is needed. Revisit if a model needs its `Driver` ergonomics. |

All pinned artifacts are at least 7 days old at adoption (youngest: Quint
0.33.1, 20 days; Apalache 0.62.4, 36 days; Kani 0.68.0, 62 days).

## Install commands (exact)

macOS (this machine). The shell `npm` function in this user's zsh is broken,
so call the binary directly.

```sh
# Java for Apalache (keg-only: not on PATH; formal/scripts/_env.sh finds it)
brew install openjdk@21

# Quint
/opt/homebrew/bin/npm i -g @informalsystems/quint@0.33.1
quint --version                       # 0.33.1

# Apalache 0.62.4 and the Rust evaluator are fetched automatically by Quint on
# the first `quint verify` / `quint run`; verify the digests above afterwards:
shasum -a 256 ~/.quint/apalache-dist-0.62.4/apalache/lib/apalache.jar \
              ~/.quint/rust-evaluator-v0.7.1/quint_evaluator

# Kani. rustup is needed only for `cargo kani setup`, which installs the
# nightly above. Keep rustup off the default PATH (keg-only) so it does not
# shadow the repository's pinned rustc 1.98.1:
brew install rustup
PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" cargo install --locked kani-verifier --version 0.68.0
PATH="/opt/homebrew/opt/rustup/bin:$HOME/.cargo/bin:$PATH" cargo kani setup
cargo kani --version                  # Kani Rust Verifier 0.68.0 (cargo plugin), CBMC 6.11.0
```

CI (Linux, `.github/workflows/formal.yml`) uses the same pins: Node with
`npm i -g @informalsystems/quint@0.33.1`, Temurin 21 (or distro OpenJDK 21),
`cargo install --locked kani-verifier --version 0.68.0` and `cargo kani setup`.
Quint downloads Apalache itself; CI should cache `~/.quint` and `~/.kani`
keyed on these versions, and check the Apalache jar digest above.

## Why each is dev-only

- **Quint / Apalache**: check protocol models in `formal/quint/` and emit ITF
  traces. Output is text (models, JSON traces, reports) committed to the repo.
  Nothing from them is built into the product.
- **Kani**: runs `cargo kani` over `#[cfg(kani)]` harnesses. The crates only
  carry `cfg(kani)` code that is compiled out of normal builds
  (`cargo::rustc-check-cfg=cfg(kani)` in `build.rs`, per 80 section 6); the
  `kani` crate is not a dependency of any product crate.
- **JDK, rustup, the Kani nightly**: runtime of the tools above on a developer
  machine or CI runner only.
- **quint-connect**: not adopted (see the table); trace replay uses `serde_json`.

## Running

```sh
formal/scripts/formal-typecheck     # quint typecheck every model (< 1 min)
formal/scripts/formal-simulate      # quint run, 10,000 traces per model, fixed seed
formal/scripts/formal-verify-fast   # quint verify (Apalache), default bounds
formal/scripts/formal-traces jobs jobs 8 30   # regenerate committed ITF samples (with --mbt metadata)
formal/scripts/formal-traces jobs jobs 10000 30 0x464d /tmp/fm1-traces --no-mbt   # bulk set for the conformance run
```

`quint verify` talks to one shared Apalache server per machine, so two `quint verify` processes at the same time
drop each other's connection ("Connection dropped"). Run them one after another (the wrappers do).

The wrappers read each model's `// formal: main=... inv=... steps=...
verify-steps=... witnesses="..."` header line, so a new model joins every job
by carrying that line. `FORMAL_SAMPLES`, `FORMAL_SEED`, `FORMAL_JAVA_HOME`,
`FORMAL_NODE_BIN` override defaults. Apalache heap is capped at 8 GB
(`JVM_ARGS=-Xmx8g`).

## Disk and host notes

`df -g /` showed 165 GB available (threshold 20 GB). The toolchain adds about
1.5 GB (Kani bundle and nightly about 1 GB, Apalache about 150 MB, JDK about
300 MB).
