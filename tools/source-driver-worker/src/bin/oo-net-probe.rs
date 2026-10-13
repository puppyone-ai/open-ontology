//! Route probe for the network-none harness.
//!
//! This is not a source driver, not an Oracle client, and not a credential
//! store. `listen` counts TCP accepts. `tcp` prints one connect result.
//! `relay` asks a unix socket to dial. `relay-serve` dials only the single
//! allowed address. `env` prints environment keys and no values.

#![forbid(unsafe_code)]

use std::env;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::net::{TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    let mode = args.next().unwrap_or_default();
    let result = match mode.as_str() {
        "listen" => listen(&mut args),
        "tcp" => tcp(&mut args),
        "relay" => relay_client(&mut args),
        "relay-serve" => relay_serve(&mut args),
        "env" => print_env(),
        _ => Err("usage"),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(token) => {
            let _ = writeln!(io::stderr(), "oo-net-probe: {token}");
            ExitCode::from(2)
        }
    }
}

fn listen(args: &mut impl Iterator<Item = String>) -> Result<(), &'static str> {
    let port = parse_port(args.next().ok_or("usage")?)?;
    let count_path = args.next().ok_or("usage")?;
    if args.next().is_some() {
        return Err("usage");
    }
    let listener = TcpListener::bind(("0.0.0.0", port)).map_err(|_| "bind")?;
    println!("listen_ready port={port}");
    let _ = io::stdout().flush();
    for accepted in listener.incoming() {
        let mut stream = accepted.map_err(|_| "accept")?;
        record_accept(&count_path)?;
        let _ = stream.write_all(b"ok\n");
        let _ = stream.flush();
    }
    Ok(())
}

fn record_accept(path: &str) -> Result<(), &'static str> {
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|_| "count")?;
    file.write_all(b"1\n").map_err(|_| "count")?;
    Ok(())
}

fn tcp(args: &mut impl Iterator<Item = String>) -> Result<(), &'static str> {
    let addr = args.next().ok_or("usage")?;
    let port = parse_port(args.next().ok_or("usage")?)?;
    if args.next().is_some() {
        return Err("usage");
    }
    let socket = format!("{addr}:{port}")
        .to_socket_addrs()
        .map_err(|_| "addr")?
        .next()
        .ok_or("addr")?;
    match TcpStream::connect_timeout(&socket, Duration::from_secs(2)) {
        Ok(mut stream) => {
            let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
            let mut buf = [0u8; 8];
            let read = stream.read(&mut buf).unwrap_or(0);
            println!("tcp_result=ok addr={addr} port={port} read={read}");
        }
        Err(err) => {
            println!(
                "tcp_result=error addr={addr} port={port} kind={} os={}",
                kind_name(err.kind()),
                err.raw_os_error().unwrap_or(-1)
            );
        }
    }
    Ok(())
}

fn relay_client(args: &mut impl Iterator<Item = String>) -> Result<(), &'static str> {
    let socket = args.next().ok_or("usage")?;
    let addr = args.next().ok_or("usage")?;
    let port = parse_port(args.next().ok_or("usage")?)?;
    if args.next().is_some() {
        return Err("usage");
    }
    let mut unix = UnixStream::connect(&socket).map_err(|_| "socket")?;
    unix.set_read_timeout(Some(Duration::from_secs(3))).map_err(|_| "socket")?;
    unix.set_write_timeout(Some(Duration::from_secs(3))).map_err(|_| "socket")?;
    writeln!(unix, "DIAL {addr} {port}").map_err(|_| "socket")?;
    let mut buf = String::new();
    read_line(&mut unix, &mut buf)?;
    let line = buf.trim_end_matches(['\n', '\r']);
    println!("relay_result={line}");
    Ok(())
}

fn relay_serve(args: &mut impl Iterator<Item = String>) -> Result<(), &'static str> {
    let allowed_ip = args.next().ok_or("usage")?;
    let allowed_port = parse_port(args.next().ok_or("usage")?)?;
    let socket_path = args.next().ok_or("usage")?;
    if args.next().is_some() {
        return Err("usage");
    }
    if Path::new(&socket_path).exists() {
        std::fs::remove_file(&socket_path).map_err(|_| "socket")?;
    }
    let listener = UnixListener::bind(&socket_path).map_err(|_| "socket")?;
    let mut perms = std::fs::metadata(&socket_path).map_err(|_| "socket")?.permissions();
    perms.set_mode(0o666);
    std::fs::set_permissions(&socket_path, perms).map_err(|_| "socket")?;
    let allowed_addr = format!("{allowed_ip}:{allowed_port}")
        .to_socket_addrs()
        .map_err(|_| "addr")?
        .next()
        .ok_or("addr")?;
    println!("relay_ready allowed={allowed_ip}:{allowed_port}");
    let _ = io::stdout().flush();
    for accepted in listener.incoming() {
        let mut unix = accepted.map_err(|_| "accept")?;
        let _ = unix.set_read_timeout(Some(Duration::from_secs(3)));
        let _ = unix.set_write_timeout(Some(Duration::from_secs(3)));
        let mut line = String::new();
        if read_line(&mut unix, &mut line).is_err() {
            continue;
        }
        let mut parts = line.split_whitespace();
        let verb = parts.next();
        let ip = parts.next();
        let port = parts.next().and_then(|text| text.parse::<u16>().ok());
        let allowed = verb == Some("DIAL")
            && ip == Some(allowed_ip.as_str())
            && port == Some(allowed_port)
            && parts.next().is_none();
        if !allowed {
            let shown_ip = ip.unwrap_or("-");
            let shown_port = port.map(|value| value.to_string()).unwrap_or_else(|| "-".to_owned());
            println!("relay_decision=DENIED ip={shown_ip} port={shown_port}");
            let _ = io::stdout().flush();
            let _ = unix.write_all(b"DENIED\n");
            continue;
        }
        println!("relay_decision=DIAL ip={allowed_ip} port={allowed_port}");
        let _ = io::stdout().flush();
        match TcpStream::connect_timeout(&allowed_addr, Duration::from_secs(2)) {
            Ok(mut remote) => {
                let _ = remote.write_all(b"ping\n");
                let mut buf = [0u8; 8];
                let _ = remote.read(&mut buf);
                let _ = unix.write_all(b"OK\n");
            }
            Err(_) => {
                let _ = unix.write_all(b"DIAL_FAIL\n");
            }
        }
    }
    Ok(())
}

fn print_env() -> Result<(), &'static str> {
    let mut keys: Vec<String> = env::vars().map(|(key, _value)| key).collect();
    keys.sort();
    for key in keys {
        println!("env_key={key}");
    }
    Ok(())
}

fn parse_port(text: String) -> Result<u16, &'static str> {
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err("port");
    }
    text.parse().map_err(|_| "port")
}

fn read_line(stream: &mut impl Read, buf: &mut String) -> Result<(), &'static str> {
    let mut byte = [0u8; 1];
    loop {
        if buf.len() > 256 {
            return Err("line");
        }
        match stream.read(&mut byte) {
            Ok(0) => return Err("line"),
            Ok(_) if byte[0] == b'\n' => return Ok(()),
            Ok(_) => buf.push(byte[0] as char),
            Err(err) if err.kind() == io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("line"),
        }
    }
}

fn kind_name(kind: io::ErrorKind) -> &'static str {
    match kind {
        io::ErrorKind::NotFound => "not_found",
        io::ErrorKind::PermissionDenied => "permission_denied",
        io::ErrorKind::ConnectionRefused => "connection_refused",
        io::ErrorKind::ConnectionReset => "connection_reset",
        io::ErrorKind::HostUnreachable => "host_unreachable",
        io::ErrorKind::NetworkUnreachable => "network_unreachable",
        io::ErrorKind::ConnectionAborted => "connection_aborted",
        io::ErrorKind::NotConnected => "not_connected",
        io::ErrorKind::AddrInUse => "addr_in_use",
        io::ErrorKind::AddrNotAvailable => "addr_not_available",
        io::ErrorKind::BrokenPipe => "broken_pipe",
        io::ErrorKind::AlreadyExists => "already_exists",
        io::ErrorKind::WouldBlock => "would_block",
        io::ErrorKind::InvalidInput => "invalid_input",
        io::ErrorKind::InvalidData => "invalid_data",
        io::ErrorKind::TimedOut => "timed_out",
        io::ErrorKind::WriteZero => "write_zero",
        io::ErrorKind::Interrupted => "interrupted",
        io::ErrorKind::Unsupported => "unsupported",
        io::ErrorKind::UnexpectedEof => "unexpected_eof",
        io::ErrorKind::OutOfMemory => "out_of_memory",
        _ => "other",
    }
}
