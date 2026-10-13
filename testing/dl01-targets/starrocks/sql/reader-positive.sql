-- Reader SELECTs only. Markers are result labels, not data.
USE oo_dl01_sr;
SELECT '@@result dup_events';
SELECT CONCAT(
  CAST(event_id AS STRING), '|',
  CAST(bucket AS STRING), '|',
  IF(amount IS NULL, '\\N', CAST(amount AS STRING)), '|',
  IF(note IS NULL, '\\N', note)
) FROM dup_events
ORDER BY event_id, bucket, IFNULL(note, ''), IFNULL(CAST(amount AS STRING), '');

SELECT '@@result pk_accounts';
SELECT CONCAT(
  CAST(account_id AS STRING), '|',
  IF(balance IS NULL, '\\N', CAST(balance AS STRING)), '|',
  IF(nickname IS NULL, '\\N', nickname)
) FROM pk_accounts
ORDER BY account_id;

SELECT '@@result agg_metrics';
SELECT CONCAT(
  CAST(metric_day AS STRING), '|',
  metric_name, '|',
  CAST(qty AS STRING), '|',
  IF(revenue IS NULL, '\\N', CAST(revenue AS STRING))
) FROM agg_metrics
ORDER BY metric_day, metric_name;

SELECT '@@result uniq_dim';
SELECT CONCAT(
  CAST(dim_id AS STRING), '|',
  IF(label IS NULL, '\\N', label), '|',
  IF(score IS NULL, '\\N', CAST(score AS STRING))
) FROM uniq_dim
ORDER BY dim_id;
