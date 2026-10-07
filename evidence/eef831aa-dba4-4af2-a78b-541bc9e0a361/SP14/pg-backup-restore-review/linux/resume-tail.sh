#!/bin/bash
# Continue the independent oracle after the copy-directory failure.
# Does not rebuild, does not start PostgreSQL, and does not repeat the backup.
set -euo pipefail
RUN_ID=eef831aa-dba4-4af2-a78b-541bc9e0a361
REPO=/Users/redux/puppy-one/open-ontology
EVIDENCE=${REPO}/evidence/${RUN_ID}/SP14/pg-backup-restore-review
ROOT=/tmp/oo-a361-${RUN_ID}
PGNAME=oo-a361-pg
BIN=/var/lib/postgresql/a361-work/pg-backup-restore
OPERATOR=/run-in/operator.json
export A361_RESUME_FROM_COPY=1
# shellcheck source=live-phases.sh
source "$EVIDENCE/linux/live-phases.sh"
