-- Owned extensions for the W39 driver preflight. The approved preparation SQL is not edited.
USE oo_synth;

CREATE TABLE k_empty (
  id INT NOT NULL,
  note VARCHAR(32) NULL,
  blob_key VARBINARY(8) NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_collation (
  id INT NOT NULL,
  bin_text VARCHAR(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_bin NOT NULL,
  uca_text VARCHAR(32) CHARACTER SET utf8mb4 COLLATE utf8mb4_uca1400_ai_ci NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_float (
  id INT NOT NULL,
  measure FLOAT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_float_key (
  id FLOAT NOT NULL,
  label VARCHAR(16) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

CREATE TABLE k_bit (
  id INT NOT NULL,
  bits BIT(8) NOT NULL,
  PRIMARY KEY (id)
) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_bin;

INSERT INTO k_empty (id, note, blob_key) VALUES
  (1, NULL, NULL),
  (2, '', x''),
  (3, 'x', x'00');

INSERT INTO k_collation (id, bin_text, uca_text) VALUES
  (1, 'A', 'A'),
  (2, 'a', 'a'),
  (3, '名', '名');

INSERT INTO k_float (id, measure) VALUES
  (1, 1.5);

INSERT INTO k_float_key (id, label) VALUES
  (1.5, 'unsafe');

INSERT INTO k_bit (id, bits) VALUES
  (1, b'10101010');
