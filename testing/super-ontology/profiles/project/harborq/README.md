# Harbor Queue

Harbor Queue is a durable job queue for small services. Jobs live in one SQLite file, workers pull them in order, and failed jobs retry with backoff.

## Quick start

```bash
cargo add harborq-core
pip install harborq-tools
```

```rust
let queue = Queue::open("jobs.db")?;
queue.push("send-email", payload)?;
```

## Usage notes

Always call Queue::close before the process exits so pending writes are flushed.

The Python tools in tools/ export and report on the queue. The dashboard in web/ shows queue depth.

## Contributing

See CONTRIBUTING.md. The license is Apache-2.0.
