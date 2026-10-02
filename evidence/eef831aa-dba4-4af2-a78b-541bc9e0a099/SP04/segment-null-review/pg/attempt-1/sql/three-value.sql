CREATE TEMP TABLE old_seg (segment_index integer, segment_count integer, segment_bitmap bytea);
ALTER TABLE old_seg ADD CONSTRAINT old_seg_check CHECK (( (segment_count IS NULL AND segment_index IS NULL AND octet_length(segment_bitmap) = 0) OR (segment_count >= 1 AND segment_index >= 0 AND segment_index < segment_count AND octet_length(segment_bitmap) BETWEEN 1 AND 8192) ));
CREATE TEMP TABLE new_seg (segment_index integer, segment_count integer, segment_bitmap bytea);
ALTER TABLE new_seg ADD CONSTRAINT new_seg_check CHECK ((( (segment_count IS NULL AND segment_index IS NULL AND octet_length(segment_bitmap) = 0) OR (segment_count >= 1 AND segment_index >= 0 AND segment_index < segment_count AND octet_length(segment_bitmap) BETWEEN 1 AND 8192) )) IS TRUE);
CREATE TEMP TABLE tv_result (line text);
INSERT INTO old_seg VALUES (NULL::integer, 1, '\x01'::bytea);
SELECT 'OLD|mix-index|' || count(*)::text FROM old_seg;
DELETE FROM old_seg;
INSERT INTO old_seg VALUES (0, NULL::integer, '\x01'::bytea);
SELECT 'OLD|mix-count|' || count(*)::text FROM old_seg;
DELETE FROM old_seg;
INSERT INTO old_seg VALUES (NULL::integer, NULL::integer, '\x01'::bytea);
SELECT 'OLD|mix-both|' || count(*)::text FROM old_seg;
DELETE FROM old_seg;
INSERT INTO old_seg VALUES (NULL::integer, NULL::integer, NULL::bytea);
SELECT 'OLD|null-bitmap|' || count(*)::text FROM old_seg;
DELETE FROM old_seg;
DO $tv$
BEGIN
  BEGIN
    INSERT INTO new_seg VALUES (NULL::integer, 1, '\x01'::bytea);
    INSERT INTO tv_result VALUES ('accepted|mix-index');
  EXCEPTION WHEN SQLSTATE '23514' THEN
    INSERT INTO tv_result VALUES ('rejected|mix-index');
  END;
  BEGIN
    INSERT INTO new_seg VALUES (0, NULL::integer, '\x01'::bytea);
    INSERT INTO tv_result VALUES ('accepted|mix-count');
  EXCEPTION WHEN SQLSTATE '23514' THEN
    INSERT INTO tv_result VALUES ('rejected|mix-count');
  END;
  BEGIN
    INSERT INTO new_seg VALUES (NULL::integer, NULL::integer, '\x01'::bytea);
    INSERT INTO tv_result VALUES ('accepted|mix-both');
  EXCEPTION WHEN SQLSTATE '23514' THEN
    INSERT INTO tv_result VALUES ('rejected|mix-both');
  END;
  BEGIN
    INSERT INTO new_seg VALUES (NULL::integer, NULL::integer, NULL::bytea);
    INSERT INTO tv_result VALUES ('accepted|null-bitmap');
  EXCEPTION WHEN SQLSTATE '23514' THEN
    INSERT INTO tv_result VALUES ('rejected|null-bitmap');
  END;
END
$tv$;
SELECT line FROM tv_result ORDER BY line;
