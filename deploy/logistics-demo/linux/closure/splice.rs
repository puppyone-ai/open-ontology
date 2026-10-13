//! Byte copy from the container loopback to the parent model forwarder.
//! Counts accepted connections. Does not read or print a token.

use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::thread;

fn main() {
    let listen = env::var("SPLICE_LISTEN").unwrap_or_else(|_| "127.0.0.1:18317".to_owned());
    let upstream = env::var("SPLICE_UPSTREAM").unwrap_or_else(|_| "host.docker.internal:8317".to_owned());
    let counter = env::var("SPLICE_COUNTER").unwrap_or_else(|_| "/tmp/splice-connects".to_owned());
    let listener = TcpListener::bind(&listen).unwrap_or_else(|_| std::process::exit(2));
    let mut accepts = 0_u64;
    for incoming in listener.incoming() {
        let Ok(client) = incoming else {
            continue;
        };
        accepts = accepts.saturating_add(1);
        let _ = fs::write(&counter, format!("{accepts}\n"));
        let Ok(remote) = TcpStream::connect(&upstream) else {
            continue;
        };
        let Ok(client_out) = client.try_clone() else {
            continue;
        };
        let Ok(remote_out) = remote.try_clone() else {
            continue;
        };
        thread::spawn(move || {
            let _ = copy(remote_out, client_out);
        });
        let _ = copy(client, remote);
    }
}

fn copy(mut from: TcpStream, mut to: TcpStream) -> io::Result<()> {
    let mut buf = [0_u8; 8192];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            let _ = to.shutdown(std::net::Shutdown::Write);
            return Ok(());
        }
        to.write_all(&buf[..n])?;
    }
}
