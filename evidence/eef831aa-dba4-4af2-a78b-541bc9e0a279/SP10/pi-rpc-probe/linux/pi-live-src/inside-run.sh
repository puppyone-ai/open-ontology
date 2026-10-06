#!/bin/bash
# Run the observation probe against the real Pi binary. Network none.
set -u
export HOME=/var/pi-home
MODE=$(cat /evidence/mode.txt)
/opt/probe/observe "$MODE" \
  /opt/pi-prefix/install/node_modules/.bin/pi \
  /opt/pi-prefix/node/bin \
  /var/pi-home \
  /evidence \
  /opt/probe/mock_openai \
  > /evidence/observe.stdout 2> /evidence/observe.stderr
echo $? > /evidence/observe.exit
echo "PHASE observe_exit=$(cat /evidence/observe.exit)"
exit 0
