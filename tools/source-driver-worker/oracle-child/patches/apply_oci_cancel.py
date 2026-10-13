#!/usr/bin/env python3
"""Overlay a caller-triggered OCI interrupt on oracledb 26.0.0-beta.4.

The published crate checksum is d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397.
Cargo.lock keeps that checksum. It is not the identity of the patched tree.
The version tag stays 26.0.0-beta.4. The published driver has no cancel API.
This overlay adds request_cancel and arm_cancel_poll on a fresh extract.
It does not add a capability getter. A poll timeout wakes the blocked read.
The interrupt marker is sent only after the caller sets the cancel flag.
A call deadline with the flag clear stays CallTimeoutExceeded.
ErrorKind::Cancelled is the observation that the marker stopped the call.
"""
import hashlib
import pathlib
import sys

CRATE_SHA256 = "d729181747bd69ba53cde2956ea9ddc06cf09d0badaf4a9e89944b7c0b7b4397"


def replace_once(text, old, new, label):
    count = text.count(old)
    if count != 1:
        raise SystemExit(f"{label}: expected 1 match, found {count}")
    return text.replace(old, new, 1)


def patch_error(path):
    text = path.read_text()
    text = replace_once(
        text,
        "    CallTimeoutExceeded,\n",
        "    CallTimeoutExceeded,\n    /// The caller asked this worker to interrupt the running call.\n    Cancelled,\n",
        "error-kind",
    )
    text = replace_once(
        text,
        """            ErrorKind::CallTimeoutExceeded => {
                fmt.write_str("the configured call timeout was exceeded")?
            }
""",
        """            ErrorKind::CallTimeoutExceeded => {
                fmt.write_str("the configured call timeout was exceeded")?
            }
            ErrorKind::Cancelled => {
                fmt.write_str("the caller cancelled the running Oracle call")?
            }
""",
        "error-display",
    )
    text = replace_once(
        text,
        """    pub(crate) fn column_truncated(
""",
        """    pub(crate) fn cancelled() -> Error {
        Error::new(ErrorKind::Cancelled, None)
    }

    pub(crate) fn column_truncated(
""",
        "error-ctor",
    )
    path.write_text(text)


def patch_client(path):
    text = path.read_text()
    text = replace_once(
        text,
        "use std::mem;\n",
        "use std::cell::Cell;\nuse std::mem;\nuse std::sync::atomic::AtomicBool;\nuse std::sync::atomic::Ordering;\nuse std::sync::Arc;\nuse std::time::Duration;\nuse std::time::Instant;\n",
        "client-uses",
    )
    text = replace_once(
        text,
        "    temp_lobs_to_close: Vec<Vec<u8>>,\n}\n",
        """    temp_lobs_to_close: Vec<Vec<u8>>,
    cancel_flag: Arc<AtomicBool>,
    cancel_poll: Cell<Option<Duration>>,
    call_deadline: Cell<Option<Instant>>,
    logical_timeout: Cell<Option<Duration>>,
}
""",
        "client-fields",
    )
    text = replace_once(
        text,
        """                Err(err) => {
                    if err.is_call_timeout_exceeded() {
                        let packet = self.recover_from_error(err)?;
                        return Ok((packet, false));
                    }
                    return Err(err);
                }
""",
        """                Err(err) => {
                    if err.is_call_timeout_exceeded() {
                        if self.cancel_flag.load(Ordering::SeqCst) {
                            return self.fail_cancelled();
                        }
                        // A poll wake is not a deadline. The deadline path
                        // leaves cancel_poll empty and keeps the original
                        // CallTimeoutExceeded recovery.
                        if self.cancel_poll.get().is_some() {
                            if let Some(deadline) = self.call_deadline.get() {
                                if Instant::now() < deadline {
                                    continue;
                                }
                            }
                        }
                        let packet = self.recover_from_error(err)?;
                        return Ok((packet, false));
                    }
                    return Err(err);
                }
""",
        "client-receive",
    )
    text = replace_once(
        text,
        """    fn recover_from_error(&mut self, err: Error) -> Result<Packet, Error> {
""",
        """    /// Send the OCI interrupt marker because the caller flag is set.
    /// The socket timeout is lifted so the server reset reply is not a poll wake.
    fn fail_cancelled(&mut self) -> Result<(Packet, bool), Error> {
        let poll = self.cancel_poll.get();
        self.transport
            .set_read_timeout(Some(Duration::from_secs(15)))?;
        let recovered = self
            .send_marker(constants::MARKER_TYPE_INTERRUPT)
            .and_then(|_| self.reset());
        if let Some(poll) = poll {
            let _ = self.transport.set_read_timeout(Some(poll));
        }
        recovered.map_err(|_| self.unrecoverable_error())?;
        Err(Error::cancelled())
    }

    fn recover_from_error(&mut self, err: Error) -> Result<Packet, Error> {
""",
        "client-fail",
    )
    text = replace_once(
        text,
        """    pub(crate) fn get_call_timeout(
        &self,
    ) -> Result<Option<std::time::Duration>, Error> {
        self.transport.get_read_timeout()
    }
""",
        """    pub(crate) fn get_call_timeout(
        &self,
    ) -> Result<Option<std::time::Duration>, Error> {
        Ok(self.logical_timeout.get())
    }

    pub(crate) fn cancel_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.cancel_flag)
    }

    /// Wake the blocked read on `poll` without moving the call deadline.
    pub(crate) fn arm_cancel_poll(&self, poll: Duration) -> Result<(), Error> {
        self.cancel_poll.set(Some(poll));
        self.transport.set_read_timeout(Some(poll))
    }
""",
        "client-accessors",
    )
    text = replace_once(
        text,
        """    pub(crate) fn set_call_timeout(
        &self,
        duration: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        self.transport.set_read_timeout(duration)
    }
""",
        """    pub(crate) fn set_call_timeout(
        &self,
        duration: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        self.logical_timeout.set(duration);
        self.call_deadline
            .set(duration.map(|item| Instant::now() + item));
        let socket = self.cancel_poll.get().or(duration);
        self.transport.set_read_timeout(socket)
    }
""",
        "client-set-timeout",
    )
    text = replace_once(
        text,
        "            temp_lobs_to_close: Vec::new(),\n        }\n    }\n",
        """            temp_lobs_to_close: Vec::new(),
            cancel_flag: Arc::new(AtomicBool::new(false)),
            cancel_poll: Cell::new(None),
            call_deadline: Cell::new(None),
            logical_timeout: Cell::new(None),
        }
    }
""",
        "client-new",
    )
    path.write_text(text)


def patch_conn(path):
    text = path.read_text()
    text = replace_once(
        text,
        """pub(crate) struct ConnImpl {
    client_ref: ClientRef,
    db_info: DbInfo,
    returned_to_pool: Instant,
}
""",
        """pub(crate) struct ConnImpl {
    client_ref: ClientRef,
    cancel_flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    db_info: DbInfo,
    returned_to_pool: Instant,
}
""",
        "conn-struct",
    )
    text = replace_once(
        text,
        """        let client_ref = std::sync::Arc::new(std::sync::Mutex::new(client));
        Ok(ConnImpl {
            client_ref,
            db_info,
            returned_to_pool: Instant::now(),
        })
""",
        """        let cancel_flag = client.cancel_flag();
        let client_ref = std::sync::Arc::new(std::sync::Mutex::new(client));
        Ok(ConnImpl {
            client_ref,
            cancel_flag,
            db_info,
            returned_to_pool: Instant::now(),
        })
""",
        "conn-connect",
    )
    text = replace_once(
        text,
        """    pub fn set_call_timeout(
        &self,
        duration: Option<Duration>,
    ) -> Result<(), Error> {
        self.client_ref.lock()?.set_call_timeout(duration)
    }
""",
        """    pub fn set_call_timeout(
        &self,
        duration: Option<Duration>,
    ) -> Result<(), Error> {
        self.client_ref.lock()?.set_call_timeout(duration)
    }

    /// Store the caller flag. This does not lock the client mutex.
    pub(crate) fn request_cancel(&self) {
        self.cancel_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }

    pub(crate) fn arm_cancel_poll(
        &self,
        poll: Duration,
    ) -> Result<(), Error> {
        self.client_ref.lock()?.arm_cancel_poll(poll)
    }
""",
        "conn-methods",
    )
    path.write_text(text)


def patch_connection(path):
    text = path.read_text()
    text = replace_once(
        text,
        """    pub fn set_call_timeout(
        &self,
        duration: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        self.get_impl()?.set_call_timeout(duration)
    }
""",
        """    pub fn set_call_timeout(
        &self,
        duration: Option<std::time::Duration>,
    ) -> Result<(), Error> {
        self.get_impl()?.set_call_timeout(duration)
    }

    /// Ask the running call to send an OCI interrupt marker.
    ///
    /// Safe to call from another thread: it only stores an atomic flag.
    /// The published 26.0.0-beta.4 crate does not have this method.
    /// This is not a capability getter. A later `ErrorKind::Cancelled`
    /// is the only observation that the marker stopped the call.
    pub fn request_cancel(&self) {
        if let Ok(inner) = self.get_impl() {
            inner.request_cancel();
        }
    }

    /// After `set_call_timeout`, wake the socket on `poll` until the deadline.
    pub fn arm_cancel_poll(
        &self,
        poll: std::time::Duration,
    ) -> Result<(), Error> {
        self.get_impl()?.arm_cancel_poll(poll)
    }
""",
        "connection-api",
    )
    path.write_text(text)


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: apply_oci_cancel.py CRATE_FILE ROOT")
    crate = pathlib.Path(sys.argv[1])
    root = pathlib.Path(sys.argv[2])
    digest = hashlib.sha256(crate.read_bytes()).hexdigest()
    if digest != CRATE_SHA256:
        raise SystemExit(f"crate checksum {digest}")
    patch_error(root / "src" / "error.rs")
    patch_client(root / "src" / "client" / "mod.rs")
    patch_conn(root / "src" / "connection" / "conn_impl.rs")
    patch_connection(root / "src" / "connection" / "mod.rs")
    stamp = root / "OO-OCI-CANCEL-OVERLAY.txt"
    stamp.write_text(
        "base_crate_sha256="
        + CRATE_SHA256
        + "\nbase_version=26.0.0-beta.4\n"
        + "overlay=caller_atomic_oci_marker_interrupt\n"
        + "deadline_error=CallTimeoutExceeded\n"
        + "cancel_error=Cancelled\n"
    )
    print("patched", root)


if __name__ == "__main__":
    main()
