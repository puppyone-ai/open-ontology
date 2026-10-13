SELECT day, region, amount
  FROM part_day PARTITION (p202601)
 ORDER BY day, region;
