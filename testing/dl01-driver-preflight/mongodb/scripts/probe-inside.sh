#!/bin/bash
# Run the prebuilt probe and decoder. This process does not invoke cargo or rustc.
set -u

printf 'prebuilt-binary\n' > /out/execution-mode.txt
if [[ -r /proc/1/cmdline ]] && tr '\0' '\n' < /proc/1/cmdline | grep -Eq '^(cargo|rustc)$'; then
  printf 'cargo-invoked\n' >> /out/execution-mode.txt
fi

set +e
export OO_W141_OPMSG_LOG=/out/opmsg-names.jsonl
export LD_PRELOAD=/binaries/liboo-opmsg-names.so
/binaries/oo-mongodb-driver-probe
printf '%s\n' "$?" > /out/probe.exit
unset LD_PRELOAD
unset OO_W141_OPMSG_LOG
/binaries/oo-mongodb-arrow-decode --arrow /out/batch.arrow.partial --out /out/decoded.json
printf '%s\n' "$?" > /out/decode.exit
/binaries/oo-mongodb-arrow-decode --arrow /out/truncated.arrow.partial --out /out/decoded-truncated.json
printf '%s\n' "$?" > /out/decode-truncated.exit
exit 0
