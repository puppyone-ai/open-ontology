from doc_parser.watchdog import EXIT_CODE, Watchdog


def test_fires_after_silence_and_not_after_a_beat():
    now = [0.0]
    calls = []
    w = Watchdog(60, exit_fn=calls.append, clock=lambda: now[0])
    now[0] = 59
    assert not w.check() and calls == []
    w.beat()
    now[0] = 118
    assert not w.check()
    now[0] = 120.5
    assert w.check() and calls == [EXIT_CODE] == [70] and w.fired
