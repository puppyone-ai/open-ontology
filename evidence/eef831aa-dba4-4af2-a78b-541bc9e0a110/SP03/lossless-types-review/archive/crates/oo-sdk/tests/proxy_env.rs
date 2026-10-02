//! Proxy environment variables must not move the query or appear in output.
//!
//! The poisoned variables are applied to a child process only. This test does
//! not call `set_var`.

#[path = "common/mod.rs"]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::{
    Action, Oracle, QUERY_RESPONSE_EMPTY, TOKEN, assert_query, json_response, sample_request,
};
use tokio::net::TcpListener;

const CANARY: &str = "a073-proxy-canary";

#[tokio::test]
async fn proxy_env_is_not_inherited() {
    if std::env::var_os("OO_SDK_PROXY_CHILD").is_some() {
        let ok = child_probe().await;
        println!("{}", if ok { "probe=ok" } else { "probe=fail" });
        assert!(ok);
        return;
    }

    let oracle =
        Oracle::spawn(|_, _| Action::Raw(json_response(200, "OK", QUERY_RESPONSE_EMPTY, &[])))
            .await;
    let proxy = TcpListener::bind("127.0.0.1:0").await.expect("proxy bind");
    let proxy_addr = proxy.local_addr().expect("proxy addr");
    let accepted = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&accepted);
    let proxy_task = tokio::spawn(async move {
        loop {
            let Ok((sock, _)) = proxy.accept().await else {
                break;
            };
            counted.fetch_add(1, Ordering::SeqCst);
            drop(sock);
        }
    });

    let proxy_url = format!("http://a073proxyuser:{CANARY}@{proxy_addr}");
    let exe = std::env::current_exe().expect("exe");
    let origin = oracle.origin();
    let child_proxy = proxy_url.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new(exe)
            .arg("proxy_env_is_not_inherited")
            .arg("--exact")
            .arg("--test-threads=1")
            .arg("--nocapture")
            .env_clear()
            .env("OO_SDK_PROXY_CHILD", "1")
            .env("OO_SDK_BASE", origin)
            .env("OO_SDK_TOKEN", TOKEN)
            .env("HTTP_PROXY", &child_proxy)
            .env("HTTPS_PROXY", &child_proxy)
            .env("ALL_PROXY", &child_proxy)
            .env("http_proxy", &child_proxy)
            .env("https_proxy", &child_proxy)
            .env("all_proxy", &child_proxy)
            .env("NO_PROXY", "")
            .env("RUST_BACKTRACE", "0")
            .env("RUST_LOG", "")
            .output()
    })
    .await
    .expect("child join")
    .expect("child");

    let secrets = [TOKEN, CANARY, "a073proxyuser", &proxy_url];
    assert_clean(&output.stdout, &secrets);
    assert_clean(&output.stderr, &secrets);
    if !output.status.success() {
        panic!(
            "proxy child failed {}\n{}\n{}",
            output.status,
            scrub(&output.stdout, &secrets),
            scrub(&output.stderr, &secrets)
        );
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("probe=ok"), "probe did not succeed");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        accepted.load(Ordering::SeqCst),
        0,
        "proxy received a connection"
    );
    proxy_task.abort();
    let hits = oracle.hits();
    assert_eq!(hits.len(), 1);
    assert_query(&hits[0], TOKEN, "/v1/query");
    assert_eq!(hits[0].path, "/v1/query");
    for (_, value) in &hits[0].headers {
        assert!(!value.contains(CANARY), "canary reached the origin");
    }
}

async fn child_probe() -> bool {
    let base = std::env::var("OO_SDK_BASE").unwrap_or_default();
    let token = std::env::var("OO_SDK_TOKEN").unwrap_or_default();
    let Ok(token) = oo_sdk::BearerToken::new(&token) else {
        return false;
    };
    let config = oo_sdk::ClientConfig::local_development().with_retry(oo_sdk::RetryPolicy::none());
    let Ok(client) = oo_sdk::Client::new(&base, token, config) else {
        return false;
    };
    client.query(sample_request()).await.is_ok()
}

fn assert_clean(bytes: &[u8], secrets: &[&str]) {
    let text = String::from_utf8_lossy(bytes);
    for secret in secrets {
        if secret.len() >= 4 && text.contains(secret) {
            panic!("child output contained a secret");
        }
    }
}

fn scrub(bytes: &[u8], secrets: &[&str]) -> String {
    let mut text = String::from_utf8_lossy(bytes).into_owned();
    for secret in secrets {
        if secret.len() >= 4 {
            text = text.replace(secret, "[redacted]");
        }
    }
    text
}
