import asyncio
import importlib.util
import json
from pathlib import Path
import tempfile
import unittest

spec = importlib.util.spec_from_file_location(
    'proxy', Path(__file__).with_name('mysql_fault_proxy.py'))
proxy = importlib.util.module_from_spec(spec)
spec.loader.exec_module(proxy)


class Writer:
    def __init__(self):
        self.data = bytearray()

    def write(self, data):
        self.data.extend(data)

    async def drain(self):
        await asyncio.sleep(0)


class RelayTests(unittest.IsolatedAsyncioTestCase):
    def reader(self, frames):
        reader = asyncio.StreamReader()
        reader.feed_data(b'\x01\x00\x00\x00\x0a' + frames)
        reader.feed_eof()
        return reader

    async def test_unarmed_relay_preserves_every_byte(self):
        with tempfile.TemporaryDirectory(dir='evidence') as directory:
            proxy.ARM = Path(directory) / 'arm'
            proxy.TRIGGER = Path(directory) / 'trigger'
            frame = b'\x17\x03\x03\x00\x03abc'
            writer = Writer()
            with self.assertRaises(asyncio.IncompleteReadError):
                await proxy.downstream(self.reader(frame), writer)
            self.assertEqual(writer.data, b'\x01\x00\x00\x00\x0a' + frame)
            self.assertFalse(proxy.TRIGGER.exists())

    async def test_armed_relay_withholds_an_incomplete_record_once(self):
        with tempfile.TemporaryDirectory(dir='evidence') as directory:
            proxy.ARM = Path(directory) / 'arm'
            proxy.TRIGGER = Path(directory) / 'trigger'
            proxy.ARM.touch()
            frame = b'\x17\x03\x03\x40\x00' + b'x' * 16384
            writer = Writer()
            task = asyncio.create_task(proxy.downstream(self.reader(frame * 100), writer))
            for _ in range(1000):
                if proxy.TRIGGER.exists():
                    break
                await asyncio.sleep(.001)
            self.assertTrue(proxy.TRIGGER.exists())
            marker = json.loads(proxy.TRIGGER.read_text())
            self.assertEqual(marker['withheld_tls_record_bytes'], 16383)
            self.assertGreater(marker['server_bytes'], 1_234_567)
            self.assertEqual(writer.data[-6:], b'\x17\x03\x03\x40\x00x')
            self.assertFalse(proxy.ARM.exists())
            self.assertFalse(task.done())
            task.cancel()
            with self.assertRaises(asyncio.CancelledError):
                await task


if __name__ == '__main__':
    unittest.main()
