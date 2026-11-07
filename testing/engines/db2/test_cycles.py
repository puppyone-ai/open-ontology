import unittest

from target_acceptance import validate_cycle


class CycleTests(unittest.TestCase):
    def receipt(self):
        return dict(protocol="oo.sync/v1", mode="c5", table={"name": "EVENTS"},
                    replay=False, target_write_bytes=0, new_generations=0,
                    payload_bytes=0, source_read_bytes=0)

    def test_idle_requires_exact_tables_and_zero_writes(self):
        row = self.receipt()
        self.assertEqual(validate_cycle([row], ["EVENTS"], 3), (0, 0))
        for field in ("target_write_bytes", "new_generations", "payload_bytes", "source_read_bytes"):
            with self.subTest(field=field), self.assertRaises(ValueError):
                validate_cycle([dict(row, **{field: 1})], ["EVENTS"], 3)
        for rows in ([], [row, row], [dict(row, replay=True)]):
            with self.assertRaises(ValueError):
                validate_cycle(rows, ["EVENTS"], 3)

    def test_changed_cycle_must_publish_one_generation(self):
        row = self.receipt()
        with self.assertRaises(ValueError):
            validate_cycle([row], ["EVENTS"], 2)
        row.update(target_write_bytes=20, new_generations=1, payload_bytes=20)
        self.assertEqual(validate_cycle([row], ["EVENTS"], 2), (20, 1))
