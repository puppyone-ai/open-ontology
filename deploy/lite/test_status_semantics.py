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
        library = copy.deepcopy(history)
        library['tables']['review_pending'] = [
            {'table': 'library:owned', 'scope': 'library', 'changes': ['relationship_candidates']}]
        report = lifecycle.publication_health(library, True, now)
        self.assertEqual((report['status'], report['alert']), ('degraded', 'library_review_required'))
        self.assertEqual(lifecycle.publication_health(history, False, now)['status'], 'unhealthy')
        for stamp in ('2026-11-07T11:57:59Z', None, 'invalid', '2099-01-01T00:00:00Z'):
            history['data_published_at'] = stamp
            report = lifecycle.publication_health(history, True, now)
            self.assertEqual(report['status'], 'unhealthy')
            self.assertIsNotNone(report['alert'])

    def test_unchanged_source_stays_healthy_through_verified_replays(self):
        from datetime import datetime, timezone
        now = datetime(2026, 11, 8, 12, 0, tzinfo=timezone.utc)
        history = {'data_published_at': '2026-11-01T00:00:00Z', 'data_verified_at': '2026-11-08T11:30:00Z',
                   'interval_seconds': 3600, 'grace_seconds': 60, 'cycle': {'result': 'succeeded'},
                   'tables': {'stale': [], 'review_pending': []}}
        self.assertEqual(lifecycle.publication_health(history, True, now)['status'], 'healthy')
        stale = dict(history, data_verified_at='2026-11-08T08:00:00Z')
        report = lifecycle.publication_health(stale, True, now)
        self.assertEqual(report['status'], 'unhealthy')
        self.assertEqual(report['alert'], 'data_publication_overdue')

