-- Fixture schema loaded by oo_dba. No passwords. No server defaults borrowed from MySQL.
USE oo_synth;

CREATE TABLE k_unsigned (
  id BIGINT UNSIGNED NOT NULL,
  label VARCHAR(64) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_decimal (
  id DECIMAL(38,10) NOT NULL,
  payload DECIMAL(65,30) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_null (
  id BIGINT NOT NULL,
  note VARCHAR(64) NULL,
  qty INT NULL,
  money DECIMAL(12,2) NULL,
  payload JSON NULL,
  when_at DATETIME(6) NULL,
  blob_key VARBINARY(8) NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_null_unique (
  id BIGINT NOT NULL,
  code VARCHAR(32) NULL,
  PRIMARY KEY (id),
  UNIQUE KEY uq_code (code)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_json (
  id BIGINT NOT NULL,
  doc JSON NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_temporal (
  id DATETIME(6) NOT NULL,
  d DATE NOT NULL,
  t TIME(6) NOT NULL,
  ts TIMESTAMP(6) NULL,
  note VARCHAR(64) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_binary (
  id VARBINARY(32) NOT NULL,
  label VARCHAR(64) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_zero_probe (
  id INT NOT NULL,
  d DATE NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;
