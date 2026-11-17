//! Remote originals: ranged download into the CAS (10 §4.3.3, B-S4-03).
//!
//! The object is read in 8 MiB ranges, each with `If-Match`, hashed on the way
//! and written to `cas/orig/` through the CAS's own tmp-and-rename. A 412 (the
//! object changed during the read) throws the partial file away and starts
//! again after a new `stat`; that counts as a restart here and never as a
//! parse attempt. Memory is a few ranges, whatever the object size. A cached
//! original is reused as it is (its name is its BLAKE3); `orig/` is trimmed by
//! the LRU in `cas::gc`.

use std::io::Read;
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};

use oo_connectors::objectstore::DocSourceError;

use super::anysource::{AnySource, Piece};
use crate::cas::{Cas, CasKind, Put};

/// Whole-object restarts after `Changed` before the caller gets `Changed`.
pub const MAX_RESTARTS: u32 = 3;

/// What a fetch did.
#[derive(Debug)]
pub struct Fetched {
    pub put: Put,
    /// Downloads started over because the object changed or failed its CRC64.
    pub restarts: u32,
    /// True when the original was already in the cache.
    pub from_cache: bool,
}

enum Msg {
    Data(Vec<u8>),
    Abort,
}

struct ChanReader {
    rx: Receiver<Msg>,
    cur: Vec<u8>,
    pos: usize,
}

impl Read for ChanReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        while self.pos >= self.cur.len() {
            match self.rx.recv() {
                Ok(Msg::Data(v)) => {
                    self.cur = v;
                    self.pos = 0;
                }
                Ok(Msg::Abort) => {
                    return Err(std::io::Error::other("download restarted"));
                }
                Err(_) => return Ok(0),
            }
        }
        let n = out.len().min(self.cur.len() - self.pos);
        out[..n].copy_from_slice(&self.cur[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

fn start_writer(
    cas: &Arc<Cas>,
) -> (
    SyncSender<Msg>,
    tokio::task::JoinHandle<Result<Put, crate::cas::CasError>>,
) {
    // One range in flight in the channel, one being read: bounded.
    let (tx, rx) = sync_channel::<Msg>(1);
    let cas = cas.clone();
    let h = tokio::task::spawn_blocking(move || {
        cas.put_reader(
            CasKind::Orig,
            ChanReader {
                rx,
                cur: Vec::new(),
                pos: 0,
            },
        )
    });
    (tx, h)
}

/// Make the original of `raw_key` present in `cas/orig/` and return its
/// address. With `want_b3` set (the hash of the version being parsed) a cached
/// original is returned without touching the network, and a download that
/// ends with other content is `Changed`: the object is no longer that version.
pub async fn fetch_original(
    source: &AnySource,
    cas: &Arc<Cas>,
    raw_key: &[u8],
    want_b3: Option<&str>,
) -> Result<Fetched, DocSourceError> {
    fetch_original_observed(source, cas, raw_key, want_b3, |_| {}).await
}

/// [`fetch_original`] that tells `observer` the bytes received so far in the
/// current attempt after every range (tests use it to change the object in
/// the middle of a download).
pub async fn fetch_original_observed<O: FnMut(u64) + Send>(
    source: &AnySource,
    cas: &Arc<Cas>,
    raw_key: &[u8],
    want_b3: Option<&str>,
    mut observer: O,
) -> Result<Fetched, DocSourceError> {
    if let Some(b3) = want_b3
        && cas.exists(CasKind::Orig, b3).unwrap_or(false)
        && cas.verify(CasKind::Orig, b3).is_ok()
    {
        let size = std::fs::metadata(
            cas.root()
                .join(crate::cas::object_rel_path(CasKind::Orig, b3).unwrap_or_default()),
        )
        .map_or(0, |m| m.len());
        return Ok(Fetched {
            put: Put {
                b3: b3.to_owned(),
                size,
                created: false,
                repaired: false,
            },
            restarts: 0,
            from_cache: true,
        });
    }
    let mut restarts = 0;
    loop {
        let (tx, handle) = start_writer(cas);
        let tx = Mutex::new(Some(tx));
        let handles = Mutex::new(vec![handle]);
        let cas2 = cas.clone();
        let mut received = 0u64;
        let result = source
            .stream_object(raw_key, |piece| {
                // The sink runs inside the download task; blocking on a
                // bounded channel is the back pressure.
                match piece {
                    Piece::Bytes(b) => {
                        received += b.len() as u64;
                        observer(received);
                        let guard = tx.lock().unwrap_or_else(|e| e.into_inner());
                        let sender = guard.as_ref().ok_or(DocSourceError::Io("writer is gone"))?;
                        sender
                            .send(Msg::Data(b))
                            .map_err(|_| DocSourceError::Io("cannot write the original"))
                    }
                    Piece::Restart => {
                        received = 0;
                        let mut slot = tx.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(old) = slot.take() {
                            let _ = old.send(Msg::Abort);
                        }
                        let (ntx, nh) = start_writer(&cas2);
                        *slot = Some(ntx);
                        handles.lock().unwrap_or_else(|e| e.into_inner()).push(nh);
                        Ok(())
                    }
                }
            })
            .await;
        match result {
            Ok((digest, _meta)) => {
                drop(tx.lock().unwrap_or_else(|e| e.into_inner()).take());
                let last = handles.lock().unwrap_or_else(|e| e.into_inner()).pop();
                let put = match last {
                    Some(h) => h
                        .await
                        .map_err(|_| DocSourceError::Io("writer task failed"))?
                        .map_err(|_| DocSourceError::Io("cannot write the original"))?,
                    None => return Err(DocSourceError::Io("writer is gone")),
                };
                if put.b3 != digest.b3 {
                    return Err(DocSourceError::Io("original does not match its hash"));
                }
                if want_b3.is_some_and(|w| w != put.b3) {
                    return Err(DocSourceError::Changed);
                }
                return Ok(Fetched {
                    put,
                    restarts,
                    from_cache: false,
                });
            }
            Err(e) => {
                if let Some(t) = tx.lock().unwrap_or_else(|e| e.into_inner()).take() {
                    let _ = t.send(Msg::Abort);
                }
                if e == DocSourceError::Changed && restarts < MAX_RESTARTS {
                    restarts += 1;
                    continue;
                }
                return Err(e);
            }
        }
    }
}
