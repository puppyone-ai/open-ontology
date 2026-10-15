# MySQL reference boundaries

This is an owned fixture reference, not production certification or another
source's acceptance. The retained old C5 and failed C1 namespaces remain
separate from `mysql_67fff4e8ebff9f91bee8c75e`.

The C1 reader consumes actual MySQL ROW/FULL binlog transactions from the
acknowledged GTID. Initial capture applies CDC before filling its snapshot;
later captures seed the acknowledged target mirror and apply real transactions.
Quiet captures do not advance source generation, apply position, or ACK.
Selected-table subsets and an unselected-table transaction were exercised.

Each changed cut currently materializes five complete typed shadows before
publication. Source follow-up does not reread a full source snapshot, but target
work remains proportional to the selected dataset. The reference does not prove
streaming delta throughput. Actual scale was 110,019 rows and 43 source columns;
the source-versus-target oracle compares every cell, including exact decimal,
binary, NULL, and declared MySQL zero-date mappings.

Installed migrations are immutable. The actual capture producer was compiled
with the 0037 installer source; the current Loader includes and installs 0038.
The release manifest must retain their separate source trees and install the
current Loader first. An older installer must not replace the installed schema.
The later producer build with the 0038 source was not used for capture and is
not the verified producer.

The Gateway's documented SQL subset excludes DISTINCT and GROUP BY. Original
random questions using distinct cardinalities and all resulting failures are
retained. The replacement independent source-gold bank uses supported scalar
aggregates, predicates, and a join; it does not establish DISTINCT support.
Model upstream HTTP 403, HTTP 408, and socket timeouts are retained separately
from successful data queries and verified answers. A rejected unqualified
relation is also retained; no SQL guard was loosened for the model.

Gateway cancellation was actually sent after query admission, rolled back the
query, and allowed a subsequent query with the same token. Existing completion
WAL classifies the canceled statement as `failed`; the evidence does not rename
it to `canceled`.

Publication uses the fixture scope, real role grants and publication fences,
and a real TLS physical PostgreSQL standby with replay checks before activation
and after opening. This does not satisfy customer owner authorization,
production deployment certification, runsc isolation, or remote audit collector
acceptance. Local admission and completion WAL are retained. Root independently
decides shared release and formal source/Goal state.

Shared source freezing includes inherited Doris projection and source-semantics
code that was present before the MySQL owner's changes. Preserving those actual
compiled bytes is provenance, not a claim that this task implemented or accepted
the Doris source.
