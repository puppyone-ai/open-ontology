use std::net::SocketAddr;
use std::sync::Arc;

use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, USER_AGENT, HeaderValue};
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use rustls::ServerConfig;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::time::timeout;
use tokio_rustls::TlsAcceptor;

use crate::allow::allow;
use crate::fault::Fault;
use crate::limits::Limits;
use crate::pem;
use crate::stream::assemble_sse;
use crate::token::{authorization_matches, body_has_token, Boundary};
use crate::upstream::{is_fixed_loopback, upstream_url, UPSTREAM_PORT};

const PUBLIC_CLIENT_VERSION_NAME: &str = "x-grok-client-version";
const PUBLIC_CLIENT_VERSION_VALUE: &str = "1.0.44";
const PUBLIC_USER_AGENT: &str = "xai-grok-workspace/1.0.44";

pub struct ListenIdentity {
    acceptor: TlsAcceptor,
}

fn install_provider() {
    match rustls::crypto::aws_lc_rs::default_provider().install_default() {
        Ok(()) => {}
        Err(_already) => {}
    }
}

pub fn load_identity(cert_pem: &[u8], key_pem: &[u8]) -> Result<ListenIdentity, Fault> {
    install_provider();
    let certs = pem::cert_der(cert_pem)?
        .into_iter()
        .map(CertificateDer::from)
        .collect::<Vec<_>>();
    let key_text = std::str::from_utf8(key_pem).map_err(|_| Fault::tls())?;
    let der = pem::key_der(key_pem)?;
    let key = if key_text.contains("BEGIN RSA PRIVATE KEY") {
        PrivateKeyDer::Pkcs1(rustls::pki_types::PrivatePkcs1KeyDer::from(der))
    } else {
        PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(der))
    };
    let mut server = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)
        .map_err(|_| Fault::tls())?;
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(ListenIdentity {
        acceptor: TlsAcceptor::from(Arc::new(server)),
    })
}

pub async fn serve_on(
    listener: TcpListener,
    upstream: SocketAddr,
    identity: ListenIdentity,
    boundary: Boundary,
    limits: Limits,
) -> Result<(), Fault> {
    if !upstream_allowed(upstream) || limits.max_concurrency == 0 {
        return Err(Fault::config());
    }
    let permits = Arc::new(Semaphore::new(limits.max_concurrency));
    let acceptor = identity.acceptor;
    loop {
        let (tcp, _) = listener.accept().await.map_err(|_| Fault::io())?;
        let acceptor = acceptor.clone();
        let boundary = boundary.clone();
        match permits.clone().try_acquire_owned() {
            Ok(permit) => {
                tokio::spawn(async move {
                    let _permit = permit;
                    let _ = timeout(limits.deadline, connection(tcp, acceptor, upstream, boundary, limits)).await;
                });
            }
            Err(_) => {
                tokio::spawn(async move {
                    if let Ok(mut tls) = acceptor.accept(tcp).await {
                        let _ = write_bytes(&mut tls, 503, br#"{"error":"busy"}"#).await;
                    }
                });
            }
        }
    }
}

fn upstream_allowed(addr: SocketAddr) -> bool {
    is_fixed_loopback(addr) && (cfg!(test) || addr.port() == UPSTREAM_PORT)
}

async fn connection(
    tcp: TcpStream,
    acceptor: TlsAcceptor,
    upstream: SocketAddr,
    boundary: Boundary,
    limits: Limits,
) {
    let Ok(mut tls) = acceptor.accept(tcp).await else {
        return;
    };
    match one_request(&mut tls, upstream, &boundary, limits).await {
        Ok(()) => {}
        Err(fault) if fault.token() == "close" => {}
        Err(fault) => {
            let _ = write_bytes(&mut tls, fault.http_status(), error_body(fault)).await;
        }
    }
}

async fn one_request<S>(stream: &mut S, upstream: SocketAddr, boundary: &Boundary, limits: Limits) -> Result<(), Fault>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let request = read_request(stream, limits).await?;
    if allow(&request.method, &request.path).is_none() {
        write_bytes(stream, 404, br#"{"error":"path"}"#).await?;
        return Ok(());
    }
    if request.method == "POST" {
        let content_type = request.content_type.as_deref().unwrap_or("");
        if !content_type.to_ascii_lowercase().starts_with("application/json") {
            return Err(Fault::config());
        }
    }
    let presented = request.authorization.as_deref().unwrap_or("");
    if request.duplicate_authorization || !authorization_matches(presented, &boundary.client_authorization) {
        return Err(Fault::client_token());
    }
    if body_has_token(&request.body, &boundary.client_authorization)
        || body_has_token(&request.body, &boundary.upstream_authorization)
    {
        return Err(Fault::secret());
    }
    let reply = forward(&request, upstream, boundary, limits).await?;
    write_bytes(stream, reply.status, &reply.body).await?;
    Ok(())
}

struct InRequest {
    method: String,
    path: String,
    content_type: Option<String>,
    authorization: Option<String>,
    duplicate_authorization: bool,
    body: Vec<u8>,
}

async fn read_request<S>(stream: &mut S, limits: Limits) -> Result<InRequest, Fault>
where
    S: AsyncRead + Unpin,
{
    let mut buf = Vec::new();
    let mut tmp = [0_u8; 2048];
    let header_len = loop {
        if find_header_end(&buf).is_none() && buf.len() > limits.max_header_bytes {
            return Err(Fault::close());
        }
        let read = stream.read(&mut tmp).await.map_err(|_| Fault::io())?;
        if read == 0 {
            return Err(Fault::io());
        }
        buf.extend_from_slice(&tmp[..read]);
        if buf.len() > limits.max_header_bytes.saturating_add(limits.max_request_bytes) {
            return Err(Fault::close());
        }
        if let Some(end) = find_header_end(&buf) {
            if end > limits.max_header_bytes {
                return Err(Fault::close());
            }
            break end;
        }
    };
    let mut headers = [httparse::EMPTY_HEADER; 32];
    let mut parsed = httparse::Request::new(&mut headers);
    let status = parsed.parse(&buf).map_err(|_| Fault::config())?;
    let httparse::Status::Complete(parsed_len) = status else {
        return Err(Fault::config());
    };
    if parsed_len != header_len {
        return Err(Fault::config());
    }
    let method = parsed.method.ok_or_else(Fault::config)?.to_string();
    let path = parsed.path.ok_or_else(Fault::config)?.to_string();
    let mut content_length = None;
    let mut content_type = None;
    let mut authorization = None;
    let mut duplicate_authorization = false;
    for header in parsed.headers {
        let name = header.name;
        let value = std::str::from_utf8(header.value).map_err(|_| Fault::config())?;
        if value.contains('\r') || value.contains('\n') {
            return Err(Fault::config());
        }
        if name.eq_ignore_ascii_case("transfer-encoding") {
            return Err(Fault::config());
        }
        if name.eq_ignore_ascii_case("content-length") {
            if content_length.is_some() {
                return Err(Fault::config());
            }
            content_length = Some(parse_length(value)?);
        } else if name.eq_ignore_ascii_case("content-type") {
            if content_type.is_some() || value.len() > 128 {
                return Err(Fault::config());
            }
            content_type = Some(value.to_string());
        } else if name.eq_ignore_ascii_case("authorization") {
            if authorization.is_some() {
                duplicate_authorization = true;
                continue;
            }
            if !(1..=600).contains(&value.len()) {
                return Err(Fault::config());
            }
            authorization = Some(value.to_string());
        }
    }
    let length = match (method.as_str(), content_length) {
        ("GET", Some(0)) => 0,
        ("GET", None) => 0,
        ("GET", Some(_)) => return Err(Fault::config()),
        ("POST", Some(length)) => length,
        ("POST", None) => return Err(Fault::config()),
        (_, Some(length)) => length,
        (_, None) => 0,
    };
    if length > limits.max_request_bytes {
        return Err(Fault::request_budget());
    }
    let mut body = buf[header_len..].to_vec();
    if body.len() > length {
        return Err(Fault::config());
    }
    while body.len() < length {
        let read = stream.read(&mut tmp).await.map_err(|_| Fault::io())?;
        if read == 0 {
            return Err(Fault::io());
        }
        let need = length - body.len();
        let take = read.min(need);
        body.extend_from_slice(&tmp[..take]);
        if read > take {
            return Err(Fault::config());
        }
    }
    Ok(InRequest {
        method,
        path,
        content_type,
        authorization,
        duplicate_authorization,
        body,
    })
}

fn parse_length(value: &str) -> Result<usize, Fault> {
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) || value.len() > 8 {
        return Err(Fault::config());
    }
    value.parse::<usize>().map_err(|_| Fault::config())
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|window| window == b"\r\n\r\n").map(|index| index + 4)
}

struct UpstreamReply {
    status: u16,
    body: Vec<u8>,
}

async fn forward(
    request: &InRequest,
    upstream: SocketAddr,
    boundary: &Boundary,
    limits: Limits,
) -> Result<UpstreamReply, Fault> {
    let url = upstream_url(upstream, &request.path)?;
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .retry(reqwest::retry::never())
        .no_proxy()
        .http1_only()
        .referer(false)
        .connection_verbose(false)
        .user_agent(PUBLIC_USER_AGENT)
        .pool_max_idle_per_host(0)
        .connect_timeout(limits.deadline)
        .timeout(limits.deadline)
        .build()
        .map_err(|_| Fault::upstream())?;
    let parsed = reqwest::Url::parse(&url).map_err(|_| Fault::config())?;
    let mut builder = match request.method.as_str() {
        "GET" => client.get(parsed.clone()),
        "POST" => client.post(parsed.clone()).body(request.body.clone()),
        _ => return Err(Fault::path()),
    };
    builder = builder.timeout(limits.deadline);
    if let Some(content_type) = &request.content_type {
        builder = builder.header(CONTENT_TYPE, content_type);
    }
    let mut header = HeaderValue::from_str(&boundary.upstream_authorization).map_err(|_| Fault::config())?;
    header.set_sensitive(true);
    builder = builder.header(AUTHORIZATION, header);
    builder = builder.header(PUBLIC_CLIENT_VERSION_NAME, PUBLIC_CLIENT_VERSION_VALUE);
    builder = builder.header(USER_AGENT, PUBLIC_USER_AGENT);
    let response = builder.send().await.map_err(|_| Fault::upstream())?;
    if response.url() != &parsed || (300..400).contains(&response.status().as_u16()) {
        return Err(Fault::upstream());
    }
    let status = response.status().as_u16();
    if !(100..600).contains(&status) {
        return Err(Fault::upstream());
    }
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut body = read_upstream(response, limits).await?;
    if body_has_token(&body, &boundary.client_authorization) || body_has_token(&body, &boundary.upstream_authorization)
    {
        return Err(Fault::secret());
    }
    if status == 200 && is_event_stream(&content_type) {
        body = assemble_sse(&body).map_err(|_| Fault::upstream())?;
    }
    Ok(UpstreamReply { status, body })
}

fn is_event_stream(content_type: &str) -> bool {
    content_type
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .eq_ignore_ascii_case("text/event-stream")
}

async fn read_upstream(mut response: reqwest::Response, limits: Limits) -> Result<Vec<u8>, Fault> {
    let started = std::time::Instant::now();
    let mut buf = Vec::new();
    let mut chunks = 0_usize;
    loop {
        let remaining = limits.deadline.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(Fault::upstream());
        }
        let next = timeout(remaining, response.chunk()).await;
        let chunk = match next {
            Ok(Ok(Some(chunk))) => chunk,
            Ok(Ok(None)) => break,
            Ok(Err(_)) | Err(_) => return Err(Fault::upstream()),
        };
        chunks = chunks.saturating_add(1);
        if chunks > limits.max_response_chunks || buf.len().saturating_add(chunk.len()) > limits.max_response_bytes {
            return Err(Fault::response_budget());
        }
        buf.extend_from_slice(&chunk);
    }
    Ok(buf)
}

async fn write_bytes<S>(stream: &mut S, status: u16, body: &[u8]) -> Result<(), Fault>
where
    S: AsyncWrite + Unpin,
{
    let header = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
        reason(status),
        body.len()
    );
    stream.write_all(header.as_bytes()).await.map_err(|_| Fault::io())?;
    stream.write_all(body).await.map_err(|_| Fault::io())?;
    stream.flush().await.map_err(|_| Fault::io())?;
    Ok(())
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        413 => "Payload Too Large",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "Status",
    }
}

fn error_body(fault: Fault) -> &'static [u8] {
    match fault.token() {
        "path" => br#"{"error":"path"}"#,
        "request_budget" => br#"{"error":"request_budget"}"#,
        "response_budget" => br#"{"error":"response_budget"}"#,
        "busy" => br#"{"error":"busy"}"#,
        "upstream" => br#"{"error":"upstream"}"#,
        "client_token" => br#"{"error":"client_token"}"#,
        "secret" => br#"{"error":"secret"}"#,
        _ => br#"{"error":"config"}"#,
    }
}

pub fn parse_listen(text: &str) -> Result<SocketAddr, Fault> {
    use std::net::{IpAddr, Ipv4Addr};
    let addr: SocketAddr = text.parse().map_err(|_| Fault::config())?;
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) || addr.port() == 0 {
        return Err(Fault::config());
    }
    Ok(addr)
}
