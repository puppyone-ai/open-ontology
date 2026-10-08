SELECT status
FROM jobs.enqueue(
  'eef831aa-dba4-4af2-a78b-541bc9e0a390'::uuid,
  'review_probe',
  1,
  '{"v":1,"units":1,"fail_attempts":0}',
  'jw-payload-1',
  '8489c431a95a59d2e2a18a8d252deb72996cfeea10723180e710bb37ed84b547',
  'review-race',
  clock_timestamp(),
  3
);
