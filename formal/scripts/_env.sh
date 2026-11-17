# Sourced by the formal-* wrappers. Locates the dev toolchain (80 section 6,
# formal/TOOLCHAIN.md). Dev-only: nothing here ships in a product artifact.
# Overrides: FORMAL_JAVA_HOME, FORMAL_NODE_BIN.
set -euo pipefail

FORMAL_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
export FORMAL_ROOT

# Node (Quint). Prefer an explicit override, then PATH, then any nvm install.
if [ -n "${FORMAL_NODE_BIN:-}" ]; then
  PATH="$FORMAL_NODE_BIN:$PATH"
elif ! command -v quint >/dev/null 2>&1; then
  for d in "$HOME"/.nvm/versions/node/*/bin; do
    if [ -x "$d/quint" ]; then PATH="$d:$PATH"; fi
  done
fi

# Java 17+ (Apalache). Explicit override, then the Homebrew keg, then PATH.
if [ -n "${FORMAL_JAVA_HOME:-}" ]; then
  JAVA_HOME="$FORMAL_JAVA_HOME"
elif [ -x /opt/homebrew/opt/openjdk@21/bin/java ]; then
  JAVA_HOME=/opt/homebrew/opt/openjdk@21
fi
if [ -n "${JAVA_HOME:-}" ]; then export JAVA_HOME; PATH="$JAVA_HOME/bin:$PATH"; fi
export PATH

# Apalache JVM heap is capped at 8 GB by policy.
export JVM_ARGS="${JVM_ARGS:--Xmx8g}"

command -v quint >/dev/null 2>&1 || { echo "quint not found; see formal/TOOLCHAIN.md" >&2; exit 127; }

# Model registry: every formal/quint/*.qnt carries one header line
#   // formal: main=<module> inv=<invariant> steps=<n> verify-steps=<n> witnesses="a b" [vinv="i1 i2 ..."]
# `inv` is the combined invariant used by simulation; `vinv` (default: inv) lists the invariants
# `quint verify` checks one by one (Apalache is far faster per invariant than on one big conjunction).
# formal_models prints "file<TAB>main<TAB>inv<TAB>steps<TAB>verify-steps<TAB>witnesses<TAB>vinv".
formal_models() {
  local f line main inv steps vsteps wit vinv
  for f in "$FORMAL_ROOT"/formal/quint/*.qnt; do
    [ -e "$f" ] || continue
    line="$(grep -m1 '^// formal:' "$f" || true)"
    [ -n "$line" ] || { echo "missing '// formal:' header in $f" >&2; return 1; }
    main="$(sed -n 's/.* main=\([^ ]*\).*/\1/p' <<<"$line")"
    inv="$(sed -n 's/.* inv=\([^ ]*\).*/\1/p' <<<"$line")"
    steps="$(sed -n 's/.* steps=\([^ ]*\).*/\1/p' <<<"$line")"
    vsteps="$(sed -n 's/.* verify-steps=\([^ ]*\).*/\1/p' <<<"$line")"
    wit="$(sed -n 's/.* witnesses="\([^"]*\)".*/\1/p' <<<"$line")"
    vinv="$(sed -n 's/.* vinv="\([^"]*\)".*/\1/p' <<<"$line")"
    printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' "$f" "$main" "$inv" "${steps:-40}" "${vsteps:-10}" "$wit" "${vinv:-$inv}"
  done
}
