SELECT day, region, amount
  FROM part_day PARTITION (p202602)
 ORDER BY day, region;
