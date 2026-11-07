import copy
from datetime import datetime, timezone
import unittest
import lifecycle


class StatusTests(unittest.TestCase):
    def test_health_uses_publication_age_not_last_cycle(self):
        now = datetime(2026, 11, 7, 12, tzinfo=timezone.utc)
        history = dict(data_published_at='2026-11-07T11:58:00Z',
                       interval_seconds=30, grace_seconds=60,
                       cycle={'result': 'succeeded'}, tables={'stale': [], 'review_pending': []})
        for result, status in [('succeeded', 'healthy'), ('failed', 'healthy'),
                               ('review_required', 'healthy'), ('partial', 'degraded')]:
            history['cycle']['result'] = result
            report = lifecycle.publication_health(history, True, now)
            self.assertEqual(report['status'], status)
            self.assertIsNone(report['alert'])
        history['cycle']['result'] = 'succeeded'
        for key in ('stale', 'review_pending'):
            changed = copy.deepcopy(history)
            changed['tables'][key] = [{'table': 'app.items'}]
            self.assertEqual(lifecycle.publication_health(changed, True, now)['status'], 'degraded')
        self.assertEqual(lifecycle.publication_health(history, False, now)['status'], 'unhealthy')
        for stamp in ('2026-11-07T11:57:59Z', None, 'invalid', '2099-01-01T00:00:00Z'):
            history['data_published_at'] = stamp
            report = lifecycle.publication_health(history, True, now)
            self.assertEqual(report['status'], 'unhealthy')
            self.assertIsNotNone(report['alert'])
