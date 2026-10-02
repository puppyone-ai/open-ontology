-- Synthetic PG 17.10 fixture for the P5 parser spike.
-- Bootstrap runs as the image superuser. This is not a production source write.
-- agent_reader is LOGIN only inside this unpublished container so probes
-- connect as that role without sending SET. Production roles stay NOLOGIN.

CREATE SCHEMA analytics;

CREATE TABLE analytics.orders (
    id integer PRIMARY KEY,
    region text NOT NULL,
    amount numeric NOT NULL,
    created_at timestamptz NOT NULL
);

CREATE TABLE analytics.customer (
    id integer PRIMARY KEY,
    name text NOT NULL,
    email text NOT NULL
);

CREATE TABLE analytics.big_fact (
    id integer PRIMARY KEY,
    ts timestamptz NOT NULL,
    amount numeric NOT NULL
);

CREATE TABLE analytics.empty_vis (
    id integer PRIMARY KEY,
    secret text NOT NULL
);

INSERT INTO analytics.orders (id, region, amount, created_at) VALUES
    (1, 'eu', 10, '2026-01-01T00:00:00Z'),
    (2, 'us', 20, '2026-01-02T00:00:00Z'),
    (3, 'eu', 5, '2026-01-03T00:00:00Z');

INSERT INTO analytics.customer (id, name, email) VALUES
    (1, 'ada', 'ada@example.test'),
    (2, 'bob', 'bob@example.test');

INSERT INTO analytics.big_fact (id, ts, amount) VALUES
    (1, '2026-01-01T00:00:00Z', 7);

INSERT INTO analytics.empty_vis (id, secret) VALUES
    (1, 'hidden');

CREATE ROLE agent_reader LOGIN;
GRANT USAGE ON SCHEMA analytics TO agent_reader;
GRANT SELECT (id, region, amount) ON analytics.orders TO agent_reader;
GRANT SELECT (id, name) ON analytics.customer TO agent_reader;
GRANT SELECT (id, ts) ON analytics.big_fact TO agent_reader;
