"""Main-loop watchdog (10 §2.6): no heartbeat for `timeout_s` -> exit 70, compose restarts us."""
import os
import threading
import time

EXIT_CODE = 70


class Watchdog:
    def __init__(self, timeout_s, exit_fn=os._exit, clock=time.monotonic):
        self.timeout_s = timeout_s
        self._exit = exit_fn
        self._clock = clock
        self._last = clock()
        self._stop = threading.Event()
        self.fired = False

    def beat(self):
        self._last = self._clock()

    def check(self):
        if self._clock() - self._last > self.timeout_s:
            self.fired = True
            self._exit(EXIT_CODE)
            return True
        return False

    def start(self):
        def run():
            step = max(0.05, min(5.0, self.timeout_s / 4))
            while not self._stop.wait(step):
                if self.check():
                    return

        threading.Thread(target=run, name="watchdog", daemon=True).start()

    def stop(self):
        self._stop.set()
