//! Byte copy from 127.0.0.1:8317 to host.docker.internal:8317.
//! pg-builder's generate origin is fixed at the loopback port.

use std::io;
use std::net::{TcpListener, TcpStream};
use std::thread;

fn main() {
    let listener = match TcpListener::bind("127.0.0.1:8317") {
        Ok(listener) => listener,
        Err(_) => {
            eprintln!("forward: bind");
            std::process::exit(1);
        }
    };
    eprintln!("forward=127.0.0.1:8317");
    for incoming in listener.incoming() {
        let Ok(client) = incoming else {
            continue;
        };
        thread::spawn(move || {
            let _ = pump(client);
        });
    }
}

fn pump(mut client: TcpStream) -> io::Result<()> {
    let mut upstream = TcpStream::connect("host.docker.internal:8317")?;
    let mut from_upstream = upstream.try_clone()?;
    let mut back_to_client = client.try_clone()?;
    thread::spawn(move || {
        let _ = io::copy(&mut from_upstream, &mut back_to_client);
        let _ = back_to_client.shutdown(std::net::Shutdown::Write);
    });
    io::copy(&mut client, &mut upstream)?;
    let _ = upstream.shutdown(std::net::Shutdown::Write);
    Ok(())
}
