#!/bin/bash
# Offline phase. The network is disconnected before this script runs.
# No C compiler is invoked. Phase exits are recorded even when a later phase fails.
set -u
OUT=/out
exec > >(tee -a "$OUT/test-console.log") 2>&1
printf 'test-start %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
printf 'network:\n' >"$OUT/offline-network.txt"
ip route >>"$OUT/offline-network.txt" 2>&1 || true
cat /proc/net/dev >>"$OUT/offline-network.txt" 2>&1 || true

record() {
  local name="$1"
  shift
  "$@" >"$OUT/${name}.log" 2>&1
  local code=$?
  printf '%s\n' "$code" >"$OUT/${name}.exit"
  printf '%s exit %s\n' "$name" "$code"
  return 0
}

cd /src
umask 022
record helper-test cargo test --locked --offline --manifest-path testing/harness/containment-supervisor/Cargo.toml -- --test-threads=4
record helper-clippy cargo clippy --locked --offline --manifest-path testing/harness/containment-supervisor/Cargo.toml --all-targets -- -D warnings
record helper-build cargo build --locked --offline --release --manifest-path testing/harness/containment-supervisor/Cargo.toml

copy_licenses() {
  local licdir=/src/testing/harness/containment-supervisor/provenance/licenses
  mkdir -p "$licdir"
  : >"$OUT/licenses-copied.txt"
  local lock=/src/testing/harness/containment-supervisor/Cargo.lock
  [ -f "$lock" ] || return 0
  awk '
    /^\[\[package\]\]/ {
      if (name != "" && checksum != "") print name "\t" version
      name = version = checksum = ""
      next
    }
    /^name = "/ { gsub(/"/, "", $3); name = $3; next }
    /^version = "/ { gsub(/"/, "", $3); version = $3; next }
    /^checksum = "/ { gsub(/"/, "", $3); checksum = $3; next }
    END { if (name != "" && checksum != "") print name "\t" version }
  ' "$lock" | while IFS="$(printf '\t')" read -r name version; do
    [ -n "$name" ] || continue
    local src crate member base
    src=$(find /cargo-home/registry/src -mindepth 2 -maxdepth 2 -type d -name "${name}-${version}" 2>/dev/null | head -1 || true)
    if [ -n "$src" ]; then
      find "$src" -maxdepth 1 -type f \( -iname 'LICENSE*' -o -iname 'COPYING*' -o -iname 'NOTICE*' \) -print | while read -r member; do
        base=$(basename "$member")
        cp "$member" "$licdir/${name}-${version}-${base}"
        printf '%s\n' "${name}-${version}-${base}" >>"$OUT/licenses-copied.txt"
      done
      continue
    fi
    crate=$(find /cargo-home/registry/cache -name "${name}-${version}.crate" 2>/dev/null | head -1 || true)
    if [ -z "$crate" ]; then
      printf 'MISSING %s-%s\n' "$name" "$version" >>"$OUT/licenses-copied.txt"
      continue
    fi
    tar -tzf "$crate" | grep -E '(^|/)(LICENSE|COPYING|NOTICE)([^/]*)$' | while read -r member; do
      base=$(basename "$member")
      tar -xOzf "$crate" "$member" >"$licdir/${name}-${version}-${base}"
      printf '%s\n' "${name}-${version}-${base}" >>"$OUT/licenses-copied.txt"
    done
  done
  if [ -f /src/LICENSE ]; then
    cp /src/LICENSE "$licdir/containment-supervisor-NOTICE-APACHE.txt"
    printf '%s\n' containment-supervisor-NOTICE-APACHE.txt >>"$OUT/licenses-copied.txt"
  fi
  if [ -f /src/NOTICE ]; then
    cp /src/NOTICE "$licdir/containment-supervisor-NOTICE.txt"
    printf '%s\n' containment-supervisor-NOTICE.txt >>"$OUT/licenses-copied.txt"
  fi
  find "$licdir" -type f -print | sort >"$OUT/license-files.txt"
}
copy_licenses

pin_code=0
bin=/target/release/containment-supervisor
if [ ! -f "$bin" ]; then
  echo 'release binary missing' >"$OUT/pin.log"
  pin_code=1
else
  digest=$(sha256sum "$bin" | awk '{print $1}')
  {
    echo "digest=$digest"
    echo "bytes=$(wc -c < "$bin" | tr -d ' ')"
    od -An -t x1 -N 4 "$bin"
  } >"$OUT/pin.log"
  mkdir -p testing/harness/containment-supervisor/bin
  chmod 0755 testing/harness/containment-supervisor/bin
  cp "$bin" testing/harness/containment-supervisor/bin/containment-supervisor
  chmod 0755 testing/harness/containment-supervisor/bin/containment-supervisor
  printf '%s\n' "$digest" > testing/harness/containment-supervisor/provenance/artifact.sha256
  zeros=0000000000000000000000000000000000000000000000000000000000000000
  hits=$(grep -c "$zeros" crates/xtask/src/containment.rs || true)
  echo "placeholder_hits=$hits" >>"$OUT/pin.log"
  if [ "$hits" != 1 ]; then
    echo 'placeholder was not unique' >>"$OUT/pin.log"
    pin_code=1
  else
    sed -i "s/${zeros}/${digest}/" crates/xtask/src/containment.rs
    if grep -q "$digest" crates/xtask/src/containment.rs && grep -q "$digest" testing/harness/containment-supervisor/provenance/artifact.sha256; then
      echo 'pin-written' >>"$OUT/pin.log"
    else
      echo 'pin-write-failed' >>"$OUT/pin.log"
      pin_code=1
    fi
  fi
  if [ -d /src/.git ]; then
    git -C /src add -- \
      testing/harness/containment-supervisor/bin/containment-supervisor \
      testing/harness/containment-supervisor/provenance/artifact.sha256 \
      testing/harness/containment-supervisor/Cargo.lock \
      testing/harness/containment-supervisor/provenance/licenses \
      crates/xtask/src/containment.rs \
      >>"$OUT/pin.log" 2>&1 || true
    git -C /src -c user.email=a082@localhost -c user.name=a082 -c commit.gpgsign=false \
      commit -m 'pin containment supervisor in the archived snapshot' >>"$OUT/pin.log" 2>&1 || true
    git -C /src status --porcelain >>"$OUT/pin.log" 2>&1 || true
  fi
fi
printf '%s\n' "$pin_code" >"$OUT/pin.exit"
printf 'pin exit %s\n' "$pin_code"

record xtask-lib cargo test --manifest-path /src/Cargo.toml --locked --offline -p xtask --lib -- --test-threads=4
record xtask-cli cargo test --manifest-path /src/Cargo.toml --locked --offline -p xtask --test cli -- --test-threads=4
record xtask-clippy cargo clippy --manifest-path /src/Cargo.toml --locked --offline -p xtask --all-targets -- -D warnings

probe=/target/a082-probe
mkdir -p "$probe"
(
  set +e
  sup=/src/testing/harness/containment-supervisor/bin/containment-supervisor
  printf 'supervisor=%s\n' "$sup"
  if [ ! -x "$sup" ]; then
    echo 'probe_result=helper-missing'
    exit 2
  fi
  sha256sum "$sup"
  echo 'probe_compiler=none'
  field() { awk -F= -v key="$1" '$1==key {print substr($0, index($0, "=")+1)}' "$2"; }
  fail=0
  note_fail() { echo "FAIL $*"; fail=1; }
  starttime() {
    local pid=$1
    if [ ! -r "/proc/$pid/stat" ]; then
      return 0
    fi
    # mawk rejects `close` as a variable name. Default FS matches Rust split_whitespace.
    awk '{
      paren = index($0, ")");
      rest = substr($0, paren + 1);
      n = split(rest, f);
      if (n >= 20) print f[20];
    }' "/proc/$pid/stat"
  }
  foreign=$(/bin/sleep 180 >/dev/null 2>&1 & echo $!)
  sleep 0.1
  foreign_start=$(starttime "$foreign")
  echo "foreign_pid=$foreign foreign_start=$foreign_start"
  if [ -z "$foreign_start" ]; then
    note_fail "foreign sleep has no starttime"
  fi
  run_one() {
    local label=$1
    shift
    local dir="$probe/$label"
    rm -rf "$dir"
    mkdir -p "$dir"
    local before after
    before=$(date +%s%N)
    "$sup" "$dir/status" "$dir/stdout" "$dir/stderr" 16777216 -- "$@" >"$dir/sup.out" 2>"$dir/sup.err"
    local code=$?
    after=$(date +%s%N)
    echo "case=$label supervisor_exit=$code elapsed_ns=$((after - before))"
    cat "$dir/status" 2>/dev/null || echo missing
  }
  setsid_bin=
  if [ -x /usr/bin/setsid ]; then setsid_bin=/usr/bin/setsid
  elif [ -x /bin/setsid ]; then setsid_bin=/bin/setsid
  fi
  echo "setsid=$setsid_bin"
  dd_bin=/bin/dd
  if [ ! -x "$dd_bin" ]; then dd_bin=/usr/bin/dd; fi
  echo "dd=$dd_bin"
  if [ -z "$setsid_bin" ]; then
    note_fail "setsid missing"
  fi
  cat >"$probe/escape.sh" <<EOF
#!/bin/sh
pidfile=\$1
mode=\$2
if [ -z "$setsid_bin" ]; then echo NO_SETSID > "\$pidfile"; exit 9; fi
if [ "\$mode" = hold-err ]; then
  "$setsid_bin" /bin/sh -c 'printf x > "\$1"; echo \$\$ > "\$2"; cat /proc/\$\$/stat >> "\$2"; while true; do printf x >> "\$1"; printf y >&2; done' _ "\$3" "\$pidfile" >/dev/null 2>&1 &
elif [ "\$mode" = hold-out ]; then
  "$setsid_bin" /bin/sh -c 'printf x > "\$1"; echo \$\$ > "\$2"; cat /proc/\$\$/stat >> "\$2"; while true; do printf x >> "\$1"; printf y; done' _ "\$3" "\$pidfile" >/dev/null 2>&1 &
else
  "$setsid_bin" /bin/sh -c 'echo \$\$ > "\$1"; cat /proc/\$\$/stat >> "\$1"; exec /bin/sleep 121' _ "\$pidfile" >/dev/null 2>&1 &
fi
if [ "\$mode" != immediate ]; then
  i=0
  while [ ! -s "\$pidfile" ] && [ "\$i" -lt 200 ]; do i=\$((i+1)); sleep 0.01; done
fi
exit 0
EOF
  chmod 0755 "$probe/escape.sh"
  cat >"$probe/chain.sh" <<'EOF'
#!/bin/sh
pidfile=$1
depth=$2
if [ "$depth" -eq 0 ]; then
  /bin/sleep 121 >/dev/null 2>&1 &
  echo $! > "$pidfile"
  cat /proc/$!/stat >> "$pidfile"
  exit 0
fi
/bin/sh "$0" "$pidfile" $((depth - 1)) >/dev/null 2>&1 &
if [ "$depth" -eq 3 ]; then
  i=0
  while [ ! -s "$pidfile" ] && [ "$i" -lt 200 ]; do i=$((i+1)); sleep 0.01; done
fi
exit 0
EOF
  chmod 0755 "$probe/chain.sh"
  check_gone() {
    local label=$1
    local pidfile=$2
    local pid start now
    pid=$(head -n 1 "$pidfile" 2>/dev/null | tr -d ' ')
    start=$(sed -n '2p' "$pidfile" 2>/dev/null | awk '{
      paren = index($0, ")");
      rest = substr($0, paren + 1);
      n = split(rest, f);
      if (n >= 20) print f[20];
    }')
    now=$(starttime "${pid:-0}")
    echo "$label pid=${pid:-missing} recorded_start=${start:-missing} now=${now:-gone}"
    if [ -z "${pid:-}" ] || [ "${pid:-0}" -le 1 ]; then
      note_fail "$label has no recorded pid"
    elif [ -z "${start:-}" ]; then
      note_fail "$label has no recorded starttime"
    elif [ -n "$now" ] && [ "$now" = "$start" ]; then
      note_fail "$label descendant still has starttime $start"
    fi
  }
  for i in $(seq 1 15); do
    dir="$probe/immediate-$i"
    run_one "immediate-$i" /bin/sh "$probe/escape.sh" "$dir/pid" immediate
    cont=$(field containment "$dir/status")
    if [ "$cont" != verified ]; then note_fail "immediate $i containment=$cont"; fi
    check_gone "immediate-$i" "$dir/pid"
  done
  for i in $(seq 1 15); do
    dir="$probe/recorded-$i"
    run_one "recorded-$i" /bin/sh "$probe/escape.sh" "$dir/pid" recorded
    cont=$(field containment "$dir/status")
    if [ "$cont" != verified ]; then note_fail "recorded $i containment=$cont"; fi
    check_gone "recorded-$i" "$dir/pid"
  done
  run_one chain /bin/sh "$probe/chain.sh" "$probe/chain/leaf.pid" 3
  cont=$(field containment "$probe/chain/status")
  if [ "$cont" != verified ]; then note_fail "chain containment=$cont"; fi
  check_gone chain "$probe/chain/leaf.pid"
  run_one hold-out /bin/sh "$probe/escape.sh" "$probe/hold-out/writer.pid" hold-out "$probe/hold-out/side.bin"
  run_one hold-err /bin/sh "$probe/escape.sh" "$probe/hold-err/writer.pid" hold-err "$probe/hold-err/side.bin"
  for label in hold-out hold-err; do
    cont=$(field containment "$probe/$label/status")
    if [ "$cont" != verified ]; then note_fail "$label containment=$cont"; fi
    check_gone "$label" "$probe/$label/writer.pid"
    side="$probe/$label/side.bin"
    before=$(stat -c '%s %i' "$side" 2>/dev/null || echo 'missing')
    sleep 0.3
    after=$(stat -c '%s %i' "$side" 2>/dev/null || echo 'missing')
    echo "$label side before=$before after=$after"
    if [ "$before" != "$after" ] || [ "$before" = missing ]; then
      note_fail "$label side changed or missing ($before -> $after)"
    fi
  done
  dir="$probe/badproto"
  mkdir -p "$dir"
  "$sup" "$dir/status" "$dir/stdout" "$dir/stderr" 0 -- /bin/sleep 30 >"$dir/sup.out" 2>"$dir/sup.err"
  code=$?
  echo "case=badproto supervisor_exit=$code"
  cat "$dir/status" 2>/dev/null || echo missing
  if [ "$code" != 82 ]; then note_fail "badproto exit $code"; fi
  note=$(field note "$dir/status")
  if [ "$note" != protocol-malformed ]; then note_fail "badproto note=$note"; fi
  dir="$probe/term"
  mkdir -p "$dir"
  before=$(date +%s%N)
  "$sup" "$dir/status" "$dir/stdout" "$dir/stderr" 1024 -- /bin/sleep 30 >"$dir/sup.out" 2>"$dir/sup.err" &
  spid=$!
  sleep 0.2
  kill -TERM "$spid" 2>/dev/null || true
  wait "$spid"
  code=$?
  after=$(date +%s%N)
  echo "case=term supervisor_exit=$code elapsed_ns=$((after - before))"
  cat "$dir/status" 2>/dev/null || echo missing
  cont=$(field containment "$dir/status")
  if [ "$cont" != verified ]; then note_fail "term containment=$cont"; fi
  if [ $((after - before)) -gt 4000000000 ]; then note_fail "term unbounded"; fi
  run_one exact "$dd_bin" if=/dev/zero bs=1048576 count=16
  exact_size=$(stat -c '%s' "$probe/exact/stdout" 2>/dev/null || echo 0)
  echo "exact_size=$exact_size limited=$(field output_limited "$probe/exact/status") exit=$(field exit_code "$probe/exact/status")"
  if [ "$exact_size" != 16777216 ] || [ "$(field output_limited "$probe/exact/status")" != 0 ] || [ "$(field exit_code "$probe/exact/status")" != 0 ] || [ "$(field containment "$probe/exact/status")" != verified ]; then
    note_fail "exact 16MiB"
  fi
  run_one plus /bin/sh -c "\"$dd_bin\" if=/dev/zero bs=1048576 count=16; \"$dd_bin\" if=/dev/zero bs=1 count=1"
  plus_size=$(stat -c '%s' "$probe/plus/stdout" 2>/dev/null || echo 0)
  echo "plus_size=$plus_size limited=$(field output_limited "$probe/plus/status")"
  if [ "$plus_size" != 16777216 ] || [ "$(field output_limited "$probe/plus/status")" != 1 ] || [ "$(field containment "$probe/plus/status")" != verified ]; then
    note_fail "plus one byte"
  fi
  sleep 0.2
  plus_after=$(stat -c '%s %i' "$probe/plus/stdout")
  echo "plus_after=$plus_after"
  run_one overshoot "$dd_bin" if=/dev/zero bs=1048576 count=32
  over_size=$(stat -c '%s' "$probe/overshoot/stdout" 2>/dev/null || echo 0)
  echo "overshoot_size=$over_size limited=$(field output_limited "$probe/overshoot/status")"
  if [ "$over_size" != 16777216 ] || [ "$(field output_limited "$probe/overshoot/status")" != 1 ]; then
    note_fail "overshoot"
  fi
  sleep 0.2
  over_after=$(stat -c '%s' "$probe/overshoot/stdout")
  if [ "$over_after" != 16777216 ]; then note_fail "overshoot grew"; fi
  now_foreign=$(starttime "$foreign")
  echo "foreign_after=$now_foreign"
  if [ "$now_foreign" != "$foreign_start" ]; then
    note_fail "foreign sleep changed ($foreign_start -> ${now_foreign:-gone})"
  fi
  # Descendants are proven by the pid and starttime recorded in each case.
  # This probe does not read /proc/*/cmdline or signal by process name.
  if [ -n "${foreign:-}" ]; then
    kill "$foreign" 2>/dev/null || true
    wait "$foreign" 2>/dev/null || true
  fi
  if [ "$fail" -eq 0 ]; then
    echo 'probe_result=pass'
    exit 0
  fi
  echo 'probe_result=fail'
  exit 1
) >"$OUT/probe.log" 2>&1
printf '%s\n' "$?" >"$OUT/probe.exit"
printf 'probe exit %s\n' "$(cat "$OUT/probe.exit")"

printf 'test-end %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
exit 0
