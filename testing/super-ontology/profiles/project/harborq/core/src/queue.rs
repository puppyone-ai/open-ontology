use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("storage failed")]
    Storage,
}

#[derive(Serialize, Deserialize)]
pub struct Job {
    pub id: u64,
    pub kind: String,
}

pub struct Queue {
    conn: Connection,
}

impl Queue {
    pub fn open(path: &str) -> Result<Queue, QueueError> {
        let conn = Connection::open(path).map_err(|_| QueueError::Storage)?;
        Ok(Queue { conn })
    }

    pub fn push(&self, kind: &str) -> Result<u64, QueueError> {
        fn inner() {}
        inner();
        let _ = (&self.conn, kind);
        Ok(1)
    }

    pub fn close(self) {}
}

pub fn batch_insert(queue: &Queue, kinds: &[&str]) -> usize {
    kinds.iter().filter(|k| queue.push(k).is_ok()).count()
}
