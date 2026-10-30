//! Loopback TLS checks. The certificates are synthetic fixtures.
//!
//! A successful handshake here is not a Gateway deployment and not PostgreSQL.

#[path = "common/mod.rs"]
mod common;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};

use common::{
    CA_PEM, Hit, LOCALHOST_KEY, LOCALHOST_PEM, OTHER_CA_PEM, OTHER_LOCALHOST_KEY,
    OTHER_LOCALHOST_PEM, QUERY_RESPONSE_EMPTY, TOKEN, assert_query, json_response, open, read_hit,
    sample_request,
};
use rustls::ServerConfig;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

use oo_sdk::{ClientConfig, LocalError, QueryError, RetryPolicy, RootTrust};

#[tokio::test]
async fn connect_ip_preserves_tls_hostname_sni_host_and_bearer() {
    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let origin = format!("https://localhost:{}/", oracle.port);
    let client = open(
        &origin,
        TOKEN,
        trust(CA_PEM).with_connect_ip("127.0.0.1".parse().unwrap()),
    );
    client.query(sample_request()).await.expect("routed TLS");
    assert_eq!(client.query_url(), format!("{origin}v1/query"));
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1);
    assert_query(&hits[0], TOKEN, "/v1/query");
    assert_eq!(hits[0].one("host"), Some(format!("localhost:{}", oracle.port).as_str()));
    assert_eq!(*oracle.server_names.lock().unwrap(), ["localhost"]);

    // The same trusted server must reject a different URL name, even at the same IP.
    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let error = open(
        &format!("https://wrong-name.invalid:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM).with_connect_ip("127.0.0.1".parse().unwrap()),
    ).query(sample_request()).await.expect_err("TLS name mismatch");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_eq!(oracle.accepted.load(Ordering::SeqCst), 1, "override reached TLS");
    assert!(oracle.hits().is_empty(), "no bearer sent on name mismatch");
    assert_secret_free(&error);

    let oracle = tls_oracle(OTHER_LOCALHOST_PEM, OTHER_LOCALHOST_KEY).await;
    let error = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM).with_connect_ip("127.0.0.1".parse().unwrap()),
    ).query(sample_request()).await.expect_err("TLS root mismatch");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_eq!(oracle.accepted.load(Ordering::SeqCst), 1);
    assert!(oracle.hits().is_empty());
}

#[tokio::test]
async fn connect_ipv6_preserves_tls_hostname() {
    let oracle = tls_oracle_body_at(
        LOCALHOST_PEM, LOCALHOST_KEY,
        json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]), "[::1]:0",
    ).await;
    open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM).with_connect_ip("::1".parse().unwrap()),
    ).query(sample_request()).await.expect("IPv6 routed TLS");
    assert_eq!(oracle.hits().len(), 1);
    assert_eq!(*oracle.server_names.lock().unwrap(), ["localhost"]);
}

#[test]
fn connect_ip_rejects_credentials_plaintext_escape_and_numeric_url_hosts() {
    for (origin, config) in [
        ("https://user:secret@localhost", ClientConfig::production()),
        ("http://localhost", ClientConfig::local_development()),
        ("https://127.0.0.1", ClientConfig::production()),
        ("https://[::1]", ClientConfig::production()),
    ] {
        let result = oo_sdk::Client::new(
            origin, oo_sdk::BearerToken::new(TOKEN).unwrap(),
            config.with_connect_ip("192.0.2.1".parse().unwrap()),
        );
        assert!(matches!(result, Err(QueryError::Local(_))), "{origin}: {result:?}");
    }
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[tokio::test]
async fn interface_binds_loopback_and_missing_interface_never_falls_back() {
    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let origin = format!("https://localhost:{}/", oracle.port);
    let interface = if cfg!(target_os = "macos") { "lo0" } else { "lo" };
    let config = trust(CA_PEM).with_connect_ip("127.0.0.1".parse().unwrap());
    open(&origin, TOKEN, config.clone().with_interface(interface.to_owned()).unwrap())
        .query(sample_request()).await.expect("bound loopback TLS");
    let error = open(
        &origin, TOKEN, config.with_interface("oo-missing0".to_owned()).unwrap(),
    ).query(sample_request()).await.expect_err("missing interface");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_eq!(oracle.accepted.load(Ordering::SeqCst), 1, "no unbound fallback");
    assert_eq!(oracle.hits().len(), 1);
}

#[tokio::test]
async fn routed_tls_keeps_client_identity_and_custom_roots() {
    let oracle = tls_oracle_body_at_auth(
        LOCALHOST_PEM, LOCALHOST_KEY,
        json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]), "127.0.0.1:0", true,
    ).await;
    let identity = include_bytes!("fixtures/route-client.pem").to_vec();
    let config = trust(CA_PEM)
        .with_identity(identity)
        .expect("synthetic identity")
        .with_connect_ip("127.0.0.1".parse().unwrap());
    open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        config,
    )
    .query(sample_request())
    .await
    .expect("routed TLS with identity");
    assert_eq!(oracle.hits().len(), 1);

    let error = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM).with_connect_ip("127.0.0.1".parse().unwrap()),
    ).query(sample_request()).await.expect_err("client certificate is required");
    assert!(matches!(error, QueryError::Transport(_)));
    assert_eq!(oracle.hits().len(), 1, "no HTTP request without client identity");
}

#[cfg(not(any(
    target_os = "android", target_os = "fuchsia", target_os = "illumos",
    target_os = "ios", target_os = "linux", target_os = "macos",
    target_os = "solaris", target_os = "tvos", target_os = "visionos",
    target_os = "watchos",
)))]
#[test]
fn unsupported_interface_is_an_explicit_local_error() {
    let error = oo_sdk::Client::new(
        "https://localhost", oo_sdk::BearerToken::new(TOKEN).unwrap(),
        ClientConfig::production().with_interface("en0".to_owned()).unwrap(),
    ).unwrap_err();
    assert!(matches!(error, QueryError::Local(LocalError::Config(
        "interface binding is unsupported on this platform"
    ))));
}

#[tokio::test]
async fn custom_ca_accepts_localhost_and_rejects_the_wrong_root_or_name() {
    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let client = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM),
    );
    let response = client.query(sample_request()).await.expect("trusted");
    assert_eq!(response.returned_rows, 0);
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1);
    assert_query(&hits[0], TOKEN, "/v1/query");
    assert!(!format!("{client:?}").contains(TOKEN), "debug leaked");

    let oracle = tls_oracle(OTHER_LOCALHOST_PEM, OTHER_LOCALHOST_KEY).await;
    let error = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM),
    )
    .query(sample_request())
    .await
    .expect_err("wrong ca");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert!(oracle.hits().is_empty(), "http after bad ca");
    assert_secret_free(&error);

    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let error = open(
        &format!("https://127.0.0.1:{}/", oracle.port),
        TOKEN,
        trust(CA_PEM),
    )
    .query(sample_request())
    .await
    .expect_err("hostname");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_secret_free(&error);

    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let error = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        ClientConfig::production().with_retry(RetryPolicy::none()),
    )
    .query(sample_request())
    .await
    .expect_err("platform roots");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_secret_free(&error);

    let oracle = tls_oracle(LOCALHOST_PEM, LOCALHOST_KEY).await;
    let error = open(
        &format!("https://localhost:{}/", oracle.port),
        TOKEN,
        trust(OTHER_CA_PEM),
    )
    .query(sample_request())
    .await
    .expect_err("other root");
    assert!(matches!(error, QueryError::Transport(_)), "{error}");
    assert_secret_free(&error);

    let key = ClientConfig::production()
        .with_trust(RootTrust::OnlyPem(LOCALHOST_KEY.as_bytes().to_vec()))
        .expect("nonempty");
    let built = oo_sdk::Client::new(
        "https://localhost:9/",
        oo_sdk::BearerToken::new(TOKEN).expect("token"),
        key,
    );
    assert!(matches!(built, Err(QueryError::Local(LocalError::Trust))));
    assert!(
        ClientConfig::production()
            .with_trust(RootTrust::OnlyPem(Vec::new()))
            .is_err()
    );
}

#[tokio::test]
async fn tls_short_token_is_omitted_from_the_error_accessors() {
    let token = "Tok006";
    let body = serde_json::json!({
        "error": {
            "code": "SQL_PARSE_ERROR",
            "message": format!("seen {token}"),
            "hint": format!("hint {token}"),
            "retryable": false,
            "query_id": "q_abc123"
        }
    })
    .to_string();
    let oracle = tls_oracle_body(
        LOCALHOST_PEM,
        LOCALHOST_KEY,
        json_response(400, "Bad Request", &body, &[]),
    )
    .await;
    let client = open(
        &format!("https://localhost:{}/", oracle.port),
        token,
        trust(CA_PEM),
    );
    let error = client
        .query(sample_request())
        .await
        .expect_err("tls redact");
    let gateway = error.gateway().expect("gateway");
    assert!(
        gateway.status() == 400 && gateway.code() == oo_sdk::ErrorCode::SqlParseError,
        "status {}",
        gateway.status()
    );
    assert!(
        !gateway.message().contains(token) && !gateway.hint().unwrap_or("").contains(token),
        "accessor text lengths {} {}",
        gateway.message().len(),
        gateway.hint().unwrap_or("").len()
    );
    let shown = format!("{error} {error:?} {client:?}");
    assert!(
        !shown.contains(token) && std::error::Error::source(&error).is_none(),
        "shown length {}",
        shown.len()
    );
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1, "hits {}", hits.len());
    let mut expected = String::from("Bearer ");
    expected.push_str(token);
    let got = hits[0].one("authorization").unwrap_or("");
    assert!(
        got.len() == expected.len() && got.as_bytes() == expected.as_bytes(),
        "authorization length {}",
        got.len()
    );
}

struct TlsOracle {
    port: u16,
    hits: Arc<Mutex<Vec<Hit>>>,
    accepted: Arc<AtomicUsize>,
    server_names: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}

impl TlsOracle {
    fn hits(&self) -> Vec<Hit> {
        self.hits.lock().expect("hits").clone()
    }
}

impl Drop for TlsOracle {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn tls_oracle(cert_pem: &'static str, key_pem: &'static str) -> TlsOracle {
    tls_oracle_body(
        cert_pem,
        key_pem,
        json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[]),
    )
    .await
}

async fn tls_oracle_body(
    cert_pem: &'static str,
    key_pem: &'static str,
    body: Vec<u8>,
) -> TlsOracle {
    tls_oracle_body_at(cert_pem, key_pem, body, "127.0.0.1:0").await
}

async fn tls_oracle_body_at(
    cert_pem: &'static str,
    key_pem: &'static str,
    body: Vec<u8>,
    address: &str,
) -> TlsOracle {
    tls_oracle_body_at_auth(cert_pem, key_pem, body, address, false).await
}

async fn tls_oracle_body_at_auth(
    cert_pem: &'static str,
    key_pem: &'static str,
    body: Vec<u8>,
    address: &str,
    require_client: bool,
) -> TlsOracle {
    let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
    let cert = CertificateDer::from(pem_block(cert_pem, "CERTIFICATE"));
    let key = PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(pem_block(key_pem, "PRIVATE KEY")));
    let builder = if require_client {
        let mut roots = rustls::RootCertStore::empty();
        roots.add(CertificateDer::from(pem_block(
            include_str!("fixtures/route-client.pem"), "CERTIFICATE",
        ))).unwrap();
        let verifier = rustls::server::WebPkiClientVerifier::builder(Arc::new(roots))
            .build().unwrap();
        ServerConfig::builder().with_client_cert_verifier(verifier)
    } else {
        ServerConfig::builder().with_no_client_auth()
    };
    let mut config = builder
        .with_single_cert(vec![cert], key)
        .expect("oracle cert");
    config.alpn_protocols = vec![b"http/1.1".to_vec()];
    let acceptor = TlsAcceptor::from(Arc::new(config));
    let listener = TcpListener::bind(address).await.expect("bind");
    let port = listener.local_addr().expect("addr").port();
    let hits = Arc::new(Mutex::new(Vec::new()));
    let stored = Arc::clone(&hits);
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    let server_names = Arc::new(Mutex::new(Vec::new()));
    let names = Arc::clone(&server_names);
    let task = tokio::spawn(async move {
        loop {
            let Ok((tcp, _)) = listener.accept().await else {
                break;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            let Ok(mut tls) = acceptor.accept(tcp).await else {
                continue;
            };
            names.lock().unwrap().push(tls.get_ref().1.server_name().unwrap_or("").to_owned());
            let Ok(hit) = read_hit(&mut tls).await else {
                continue;
            };
            stored.lock().expect("hits").push(hit);
            let _ = tls.write_all(&body).await;
            let _ = tls.flush().await;
        }
    });
    TlsOracle { port, hits, accepted, server_names, task }
}

fn trust(pem: &str) -> ClientConfig {
    ClientConfig::production()
        .with_retry(RetryPolicy::none())
        .with_trust(RootTrust::OnlyPem(pem.as_bytes().to_vec()))
        .expect("pem")
}

fn assert_secret_free(error: &QueryError) {
    let shown = format!("{error} {error:?}");
    assert!(!shown.contains(TOKEN), "tls error leaked");
    assert!(!shown.contains("PRIVATE KEY"), "tls error leaked a key");
    assert!(!shown.to_ascii_lowercase().contains("cancel"), "{shown}");
}

fn pem_block(pem: &str, label: &str) -> Vec<u8> {
    let begin = format!("-----BEGIN {label}-----");
    let end = format!("-----END {label}-----");
    let start = pem.find(&begin).expect("pem begin") + begin.len();
    let stop = pem[start..].find(&end).expect("pem end") + start;
    let encoded: String = pem[start..stop]
        .chars()
        .filter(|ch| !ch.is_ascii_whitespace())
        .collect();
    b64_decode(&encoded).expect("pem base64")
}

fn b64_decode(input: &str) -> Option<Vec<u8>> {
    fn value(byte: u8) -> Option<u8> {
        match byte {
            b'A'..=b'Z' => Some(byte - b'A'),
            b'a'..=b'z' => Some(byte - b'a' + 26),
            b'0'..=b'9' => Some(byte - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let bytes = input.as_bytes();
    if bytes.len() & 3 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
    for chunk in bytes.chunks(4) {
        let pad = chunk.iter().filter(|byte| **byte == b'=').count();
        if pad > 2 {
            return None;
        }
        let first = value(chunk[0])?;
        let second = value(chunk[1])?;
        let third = if chunk[2] == b'=' {
            0
        } else {
            value(chunk[2])?
        };
        let fourth = if chunk[3] == b'=' {
            0
        } else {
            value(chunk[3])?
        };
        out.push((first << 2) | (second >> 4));
        if pad < 2 {
            out.push((second << 4) | (third >> 2));
        }
        if pad < 1 {
            out.push((third << 6) | fourth);
        }
    }
    Some(out)
}
