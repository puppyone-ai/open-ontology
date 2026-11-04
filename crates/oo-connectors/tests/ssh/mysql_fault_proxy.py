"""Fixture-only opaque TLS relay. Never decrypt or log database bytes."""
import asyncio
import json
from pathlib import Path

ARM = Path('/tmp/fault-arm')
TRIGGER = Path('/tmp/fault-triggered')


async def upstream(reader, writer):
    while data := await reader.read(65536):
        writer.write(data)
        await writer.drain()


async def downstream(reader, writer):
    # The server greeting is plaintext MySQL; all following replies use TLS.
    header = await reader.readexactly(4)
    greeting = await reader.readexactly(int.from_bytes(header[:3], 'little'))
    writer.write(header + greeting)
    await writer.drain()
    transferred = 0
    while True:
        header = await reader.readexactly(5)
        length = int.from_bytes(header[3:], 'big')
        if header[0] not in (20, 21, 22, 23) or length > 18432:
            raise ValueError('expected bounded TLS record')
        payload = await reader.readexactly(length)
        transferred += len(header) + length
        if ARM.exists() and header[0] == 23 and length > 1024 and transferred > 1_234_567:
            ARM.unlink()
            writer.write(header + payload[:1])
            await writer.drain()
            TRIGGER.write_text(json.dumps({
                'withheld_tls_record_bytes': length - 1,
                'server_bytes': transferred,
            }))
            # The caller cannot finish this TLS record or admit the chunk.
            # The host runner now kills/restarts only this owned jump.
            await asyncio.Future()
        writer.write(header + payload)
        await writer.drain()


async def connection(reader, writer):
    remote = None
    tasks = []
    try:
        remote_reader, remote = await asyncio.open_connection('db.origin', 3306)
        tasks = [
            asyncio.create_task(upstream(reader, remote)),
            asyncio.create_task(downstream(remote_reader, writer)),
        ]
        done, _ = await asyncio.wait(tasks, return_when=asyncio.FIRST_COMPLETED)
        for task in done:
            task.result()
    except (OSError, ValueError, asyncio.IncompleteReadError):
        pass
    finally:
        for task in tasks:
            task.cancel()
        await asyncio.gather(*tasks, return_exceptions=True)
        writer.close()
        if remote:
            remote.close()


async def main():
    server = await asyncio.start_server(connection, '0.0.0.0', 3306)
    async with server:
        await server.serve_forever()


if __name__ == '__main__':
    asyncio.run(main())
