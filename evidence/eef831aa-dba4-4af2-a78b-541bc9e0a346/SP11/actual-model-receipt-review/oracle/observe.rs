#![forbid(unsafe_code)]

//! Byte-forwarding observer in front of the reviewed relay.
//! It does not read the upstream token and it does not rewrite model or SQL.
//! Authorization values are redacted before any file is written.

use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use base64::Engine;
use oo_local_llm::{https_once, Fault, Limits};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::TlsAcceptor;

struct Options {
    listen: SocketAddr,
    cert: PathBuf,
    key: PathBuf,
    upstream_origin: String,
    ca: PathBuf,
    out: PathBuf,
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(token) => {
            eprintln!("a346-observe: {token}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<(), &'static str> {
    let options = options().map_err(|_| "config")?;
    install_provider();
    let cert = fs::read(&options.cert).map_err(|_| "tls")?;
    let key = fs::read(&options.key).map_err(|_| "tls")?;
    let ca = fs::read(&options.ca).map_err(|_| "tls")?;
    if ca.windows(b"PRIVATE KEY".len()).any(|window| window == b"PRIVATE KEY") {
        return Err("tls");
    }
    let acceptor = acceptor(&cert, &key).map_err(|_| "tls")?;
    fs::create_dir_all(&options.out).map_err(|_| "io")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .enable_time()
        .build()
        .map_err(|_| "io")?;
    runtime.block_on(serve(options, ca, acceptor))
}

fn install_provider() {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
}

async fn serve(options: Options, ca: Vec<u8>, acceptor: TlsAcceptor) -> Result<(), &'static str> {
    let listener = TcpListener::bind(options.listen).await.map_err(|_| "bind")?;
    let bound = listener.local_addr().map_err(|_| "bind")?;
    println!("listen={bound}");
    let mut posts = 0_u32;
    loop {
        let (tcp, _) = listener.accept().await.map_err(|_| "io")?;
        let acceptor = acceptor.clone();
        let ca = ca.clone();
        let origin = options.upstream_origin.clone();
        let out = options.out.clone();
        match one_connection(tcp, acceptor, &ca, &origin, &out, posts).await {
            Ok(kind) => {
                if kind == "post" {
                    posts = posts.saturating_add(1);
                }
            }
            Err(token) => {
                eprintln!("a346-observe: {token}");
            }
        }
    }
}

async fn one_connection(
    tcp: TcpStream,
    acceptor: TlsAcceptor,
    ca: &[u8],
    origin: &str,
    out: &Path,
    posts: u32,
) -> Result<&'static str, &'static str> {
    let mut tls = acceptor.accept(tcp).await.map_err(|_| "tls")?;
    let request = read_request(&mut tls).await?;
    let token = bearer_token(request.authorization.as_deref());
    if request.body.windows(b"PRIVATE KEY".len()).any(|window| window == b"PRIVATE KEY") {
        let _ = write_bytes(&mut tls, 400, br#"{"error":"secret"}"#).await;
        return Err("secret");
    }
    if let Some(secret) = token.as_deref()
        && !secret.is_empty()
        && request.body.windows(secret.len()).any(|window| window == secret.as_bytes())
    {
        let _ = write_bytes(&mut tls, 400, br#"{"error":"secret"}"#).await;
        return Err("secret");
    }
    let limits = Limits {
        max_header_bytes: 8192,
        max_request_bytes: 65536,
        max_response_bytes: 65536,
        max_response_chunks: 32,
        deadline: std::time::Duration::from_millis(30_000),
        max_concurrency: 1,
    };
    let reply = https_once(
        origin,
        &request.path,
        ca,
        token.as_deref(),
        if request.method == "POST" { Some(request.body.as_slice()) } else { None },
        limits,
    )
    .await;
    let (status, body) = match reply {
        Ok(reply) => (reply.status, reply.body),
        Err(fault) if fault == Fault::secret() => {
            let _ = write_bytes(&mut tls, 502, br#"{"error":"secret"}"#).await;
            return Err("secret");
        }
        Err(_) => {
            let _ = write_bytes(&mut tls, 502, br#"{"error":"upstream"}"#).await;
            return Err("upstream");
        }
    };
    if let Some(secret) = token.as_deref()
        && body.windows(secret.len()).any(|window| window == secret.as_bytes())
    {
        let _ = write_bytes(&mut tls, 502, br#"{"error":"secret"}"#).await;
        return Err("secret");
    }
    save(out, &request, status, &body, posts)?;
    write_bytes(&mut tls, status, &body).await?;
    let forwarded_bytes = body.len();
    println!("forwarded status={status} bytes={forwarded_bytes}");
    if request.method == "POST" { Ok("post") } else { Ok("other") }
}

struct InRequest {
    method: String,
    path: String,
    authorization: Option<String>,
    body: Vec<u8>,
}

async fn read_request(stream: &mut tokio_rustls::server::TlsStream<TcpStream>) -> Result<InRequest, &'static str> {
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 2048];
    let header_len = loop {
        if find_header_end(&buf).is_none() && buf.len() > 8192 {
            return Err("budget");
        }
        let read = stream.read(&mut tmp).await.map_err(|_| "io")?;
        if read == 0 {
            return Err("io");
        }
        buf.extend_from_slice(&tmp[..read]);
        if buf.len() > 8192 + 65536 {
            return Err("budget");
        }
        if let Some(end) = find_header_end(&buf) {
            break end;
        }
    };
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut parsed = httparse::Request::new(&mut headers);
    let httparse::Status::Complete(parsed_len) = parsed.parse(&buf).map_err(|_| "config")? else {
        return Err("config");
    };
    if parsed_len != header_len {
        return Err("config");
    }
    let method = parsed.method.ok_or("config")?.to_string();
    let path = parsed.path.ok_or("config")?.to_string();
    if path != "/v1/models" && path != "/v1/chat/completions" {
        return Err("path");
    }
    let mut authorization = None;
    let mut content_length = None;
    for header in parsed.headers {
        let value = std::str::from_utf8(header.value).map_err(|_| "config")?;
        if value.contains('\r') || value.contains('\n') {
            return Err("config");
        }
        if header.name.eq_ignore_ascii_case("authorization") {
            if authorization.is_some() {
                return Err("client_token");
            }
            authorization = Some(value.to_string());
        } else if header.name.eq_ignore_ascii_case("content-length") {
            content_length = Some(parse_length(value)?);
        }
    }
    let length = match (method.as_str(), content_length) {
        ("GET", None | Some(0)) => 0,
        ("POST", Some(length)) if length <= 65536 => length,
        _ => return Err("config"),
    };
    let mut body = buf[header_len..].to_vec();
    if body.len() > length {
        return Err("config");
    }
    while body.len() < length {
        let read = stream.read(&mut tmp).await.map_err(|_| "io")?;
        if read == 0 {
            return Err("io");
        }
        let need = length - body.len();
        let take = read.min(need);
        body.extend_from_slice(&tmp[..take]);
        if read > take {
            return Err("config");
        }
    }
    Ok(InRequest { method, path, authorization, body })
}

fn bearer_token(header: Option<&str>) -> Option<String> {
    let header = header?;
    let token = header.strip_prefix("Bearer ")?;
    if token.is_empty() || token.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
        return None;
    }
    Some(token.to_string())
}

fn save(out: &Path, request: &InRequest, status: u16, body: &[u8], posts: u32) -> Result<(), &'static str> {
    let suffix = if request.method == "POST" && posts > 0 {
        format!("-{posts}")
    } else {
        String::new()
    };
    let request_name = if request.method == "POST" {
        format!("request-body{suffix}.json")
    } else {
        format!("models-body{suffix}.json")
    };
    let response_name = if request.method == "POST" {
        format!("completion{suffix}.json")
    } else {
        format!("models-response{suffix}.json")
    };
    fs::write(out.join(&request_name), &request.body).map_err(|_| "io")?;
    fs::write(out.join(&response_name), body).map_err(|_| "io")?;
    let method = &request.method;
    let path = &request.path;
    let request_bytes = request.body.len();
    let response_bytes = body.len();
    let request_sha256 = sha256_hex(&request.body);
    let response_sha256 = sha256_hex(body);
    let note = format!(
        "method={method}\npath={path}\nstatus={status}\nrequest_bytes={request_bytes}\nresponse_bytes={response_bytes}\nrequest_sha256={request_sha256}\nresponse_sha256={response_sha256}\nauthorization=redacted\n"
    );
    let note_name = if request.method == "POST" {
        format!("exchange{suffix}.txt")
    } else {
        format!("models-exchange{suffix}.txt")
    };
    fs::write(out.join(note_name), note).map_err(|_| "io")?;
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for byte in digest {
        out.push(char::from(b"0123456789abcdef"[(byte >> 4) as usize]));
        out.push(char::from(b"0123456789abcdef"[(byte & 0x0f) as usize]));
    }
    out
}

async fn write_bytes(
    stream: &mut tokio_rustls::server::TlsStream<TcpStream>,
    status: u16,
    body: &[u8],
) -> Result<(), &'static str> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        426 => "Upgrade Required",
        502 => "Bad Gateway",
        _ => "Result",
    };
    let content_length = body.len();
    let header = format!(
        "HTTP/1.1 {status} {reason}\r\ncontent-type: application/json\r\ncontent-length: {content_length}\r\nconnection: close\r\n\r\n"
    );
    stream.write_all(header.as_bytes()).await.map_err(|_| "io")?;
    stream.write_all(body).await.map_err(|_| "io")?;
    let _ = stream.shutdown().await;
    Ok(())
}

fn acceptor(cert_pem: &[u8], key_pem: &[u8]) -> Result<TlsAcceptor, ()> {
    let certs = cert_der(cert_pem)?.into_iter().map(CertificateDer::from).collect::<Vec<_>>();
    let key_der = key_der(key_pem)?;
    let key = if std::str::from_utf8(key_pem).map_err(|_| ())?.contains("BEGIN RSA PRIVATE KEY") {
        PrivateKeyDer::Pkcs1(rustls::pki_types::PrivatePkcs1KeyDer::from(key_der))
    } else {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(key_der))
    };
    let mut server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| ())?;
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(TlsAcceptor::from(Arc::new(server)))
}

fn cert_der(pem: &[u8]) -> Result<Vec<Vec<u8>>, ()> {
    let text = std::str::from_utf8(pem).map_err(|_| ())?;
    if text.contains("PRIVATE KEY") || !text.contains("BEGIN CERTIFICATE") {
        return Err(());
    }
    let mut certs = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("-----BEGIN CERTIFICATE-----") {
        let after = &rest[start + "-----BEGIN CERTIFICATE-----".len()..];
        let end = after.find("-----END CERTIFICATE-----").ok_or(())?;
        certs.push(decode_b64(&after[..end])?);
        rest = &after[end + "-----END CERTIFICATE-----".len()..];
    }
    if certs.is_empty() { Err(()) } else { Ok(certs) }
}

fn key_der(pem: &[u8]) -> Result<Vec<u8>, ()> {
    let text = std::str::from_utf8(pem).map_err(|_| ())?;
    let label = if text.contains("BEGIN PRIVATE KEY") {
        "PRIVATE KEY"
    } else if text.contains("BEGIN RSA PRIVATE KEY") {
        "RSA PRIVATE KEY"
    } else {
        return Err(());
    };
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = text.find(&begin).ok_or(())?;
    let after = &text[start + begin.len()..];
    let stop = after.find(&end).ok_or(())?;
    decode_b64(&after[..stop])
}

fn decode_b64(body: &str) -> Result<Vec<u8>, ()> {
    let joined: String = body.chars().filter(|ch| !ch.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD.decode(joined).map_err(|_| ())
}

fn parse_length(value: &str) -> Result<usize, &'static str> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) || value.len() > 8 {
        return Err("config");
    }
    value.parse::<usize>().map_err(|_| "config")
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

fn options() -> Result<Options, ()> {
    let mut args = env::args().skip(1);
    let mut listen = None;
    let mut cert = None;
    let mut key = None;
    let mut upstream_origin = None;
    let mut ca = None;
    let mut out = None;
    while let Some(flag) = args.next() {
        let value = args.next().ok_or(())?;
        match flag.as_str() {
            "--listen" if listen.is_none() => {
                let addr: SocketAddr = value.parse().map_err(|_| ())?;
                if addr.ip().to_string() != "127.0.0.1" {
                    return Err(());
                }
                listen = Some(addr);
            }
            "--cert" if cert.is_none() => cert = Some(PathBuf::from(value)),
            "--key" if key.is_none() => key = Some(PathBuf::from(value)),
            "--upstream-origin" if upstream_origin.is_none() => {
                if !value.starts_with("https://127.0.0.1:") {
                    return Err(());
                }
                upstream_origin = Some(value);
            }
            "--ca" if ca.is_none() => ca = Some(PathBuf::from(value)),
            "--out" if out.is_none() => out = Some(PathBuf::from(value)),
            _ => return Err(()),
        }
    }
    Ok(Options {
        listen: listen.ok_or(())?,
        cert: cert.ok_or(())?,
        key: key.ok_or(())?,
        upstream_origin: upstream_origin.ok_or(())?,
        ca: ca.ok_or(())?,
        out: out.ok_or(())?,
    })
}
