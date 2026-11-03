-- Q01: How many orders exist?
SELECT COUNT(*) AS value FROM orders;

-- Q02: What is the exact total order amount?
SELECT SUM(amount) AS value FROM orders;

-- Q03: How many customers exist?
SELECT COUNT(*) AS value FROM customers;

-- Q04: How many shipments join to existing orders and customers?
SELECT COUNT(*) AS value FROM shipments s JOIN orders o ON s.order_id=o.id JOIN customers c ON o.customer_id=c.id;

-- Q05: Order amount by region through customer, site and area?
SELECT r.id,SUM(o.amount) FROM orders o JOIN customers c ON o.customer_id=c.id JOIN sites s ON c.site_id=s.id JOIN areas a ON s.area_id=a.id JOIN regions r ON a.region_id=r.id GROUP BY r.id ORDER BY r.id;

-- Q06: Order count by translated status?
SELECT c.label,COUNT(*) FROM orders o JOIN codes c ON o.status_code=c.code GROUP BY c.label ORDER BY c.label;

-- Q07: How many orders are in January 2026?
SELECT COUNT(*) FROM orders WHERE ordered_at>=DATE '2026-01-01' AND ordered_at<DATE '2026-02-01';

-- Q08: January exact amount?
SELECT SUM(amount) FROM orders WHERE ordered_at>=DATE '2026-01-01' AND ordered_at<DATE '2026-02-01';

-- Q09: How many customer notes are NULL or empty under portable source semantics?
SELECT COUNT(*) FROM customers WHERE NULLIF(note,'') IS NULL;

-- Q10: How many nonempty customer notes?
SELECT COUNT(NULLIF(note,'')) FROM customers;

-- Q11: How many orders have no shipment?
SELECT COUNT(*) FROM orders o LEFT JOIN shipments s ON s.order_id=o.id WHERE s.id IS NULL;

-- Q12: Total quantity by customer?
SELECT customer_id,SUM(qty) FROM orders GROUP BY customer_id ORDER BY customer_id;

-- Q13: Shipment counts by implicit destination site?
SELECT s.site_id,COUNT(*) FROM shipments s JOIN sites t ON s.site_id=t.id GROUP BY s.site_id ORDER BY s.site_id;

-- Q14: Composite-key order line quantity?
SELECT SUM(l.qty) FROM order_lines l JOIN orders o ON l.order_id=o.id;

-- Q15: How many distinct customers placed shipped-status orders?
SELECT COUNT(DISTINCT customer_id) FROM orders WHERE status_code=2;

-- Q16: How many calendar dates in January?
SELECT COUNT(*) FROM calendar WHERE month_no=1;

-- Q17: How many bulk events exist?
SELECT COUNT(*) FROM events;

-- Q18: How many event-to-order-to-customer relations?
SELECT COUNT(*) FROM events e JOIN orders o ON e.order_id=o.id JOIN customers c ON o.customer_id=c.id;

-- Q19: Minimum and maximum exact order amounts?
SELECT MIN(amount),MAX(amount) FROM orders;

-- Q20: How many rows across January and February shards?
SELECT COUNT(*) FROM (SELECT id FROM log_202601 UNION ALL SELECT id FROM log_202602) shard_rows;
