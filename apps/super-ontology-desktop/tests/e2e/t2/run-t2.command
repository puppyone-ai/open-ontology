#!/bin/bash
# Starts the T2 packaged run in a Terminal window and closes that window when it ends (the log stays on disk).
#   open -a Terminal tests/e2e/t2/run-t2.command
# Terminal does not pass the caller's environment on, so the options come from ~/.config/open-ontology/so-t2.args (one line of
# run-packaged.mjs options, read once and removed); without it the run is `--scenarios all` on /Applications/Super Ontology.app.
# The log goes to the path on the file's second line, or ~/Desktop/super-ontology-acceptance/.
cd "$(dirname "$0")/../../.." || exit 1
export PATH="/opt/homebrew/opt/node@24/bin:$PATH"
ARGS_FILE="$HOME/.config/open-ontology/so-t2.args"
ARGS="--scenarios all"
LOG="$HOME/Desktop/super-ontology-acceptance/t2-$(date +%Y%m%d-%H%M%S).log"
if [ -f "$ARGS_FILE" ]; then
  ARGS="$(sed -n 1p "$ARGS_FILE")"
  [ -n "$(sed -n 2p "$ARGS_FILE")" ] && LOG="$(sed -n 2p "$ARGS_FILE")"
  rm -f "$ARGS_FILE"
fi
mkdir -p "$(dirname "$LOG")"
eval "node tests/e2e/t2/run-packaged.mjs $ARGS" 2>&1 | tee "$LOG"
echo "exit ${PIPESTATUS[0]}" >> "$LOG"
osascript -e 'tell application "Terminal" to close (every window whose name contains "run-t2.command")' >/dev/null 2>&1 &
