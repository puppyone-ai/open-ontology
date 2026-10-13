-- Run only as oo_dl01w1_09_reader, in its own client.
-- The runner starts this batch, waits until the session is visible, then kills the
-- client process so the TDS connection drops. The reader is not granted KILL.
-- Expected observation, by the DBA session: the request is gone, and the reader
-- Extended Events session has an attention event. Not executed on the arm64 host.
WAITFOR DELAY '00:00:45';
SELECT id FROM dbo.oracle_row;
