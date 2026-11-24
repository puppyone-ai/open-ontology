//! The one seam between the queue adapters and the network: a request and a response as plain values.
//! The adapters build and read them; [`ReqwestTransport`] sends them (feature `objectstore-s3`, which
//! brings `reqwest`); the tests send them to a fake that plays the cloud. No adapter opens a socket
//! itself, so nothing in this module can reach a real cloud unless a caller wires a real transport.

use std::future::Future;

use crate::objectstore::DocSourceError;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: &'static str,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl HttpRequest {
    /// First header of that name (names compare ignoring case).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub trait HttpTransport: Send + Sync {
    fn send(
        &self,
        request: HttpRequest,
    ) -> impl Future<Output = Result<HttpResponse, DocSourceError>> + Send;
}

/// The closed error a status code stands for. 2xx is not an error and never reaches this.
pub fn error_of_status(status: u16) -> DocSourceError {
    match status {
        401 => DocSourceError::AuthFailed,
        403 => DocSourceError::PermissionDenied,
        404 => DocSourceError::NotFound,
        408 | 504 => DocSourceError::Timeout,
        429 => DocSourceError::Throttled {
            retry_after_ms: 1000,
        },
        500..=599 => DocSourceError::Unavailable,
        _ => DocSourceError::Protocol("queue replied unexpectedly"),
    }
}

/// `scheme://host[:port]` and `host[:port]` of a base URL; the path (if any) is dropped.
pub fn split_endpoint(endpoint: &str) -> Result<(String, String), DocSourceError> {
    let bad = DocSourceError::InvalidConfig("queue endpoint is not an http(s) origin");
    let (scheme, rest) = endpoint.split_once("://").ok_or_else(|| bad.clone())?;
    if scheme != "https" && scheme != "http" {
        return Err(bad);
    }
    let host = rest.split('/').next().unwrap_or_default();
    if host.is_empty() || host.contains('@') || host.contains(char::is_whitespace) {
        return Err(bad);
    }
    Ok((format!("{scheme}://{host}"), host.to_owned()))
}

#[cfg(feature = "objectstore-s3")]
pub use net::ReqwestTransport;

#[cfg(feature = "objectstore-s3")]
mod net {
    use std::time::Duration;

    use super::{HttpRequest, HttpResponse, HttpTransport};
    use crate::objectstore::DocSourceError;

    /// `reqwest` underneath, no redirects (a queue API never redirects, and a redirect would carry the
    /// signature to another host).
    #[derive(Clone)]
    pub struct ReqwestTransport {
        client: reqwest::Client,
    }

    impl ReqwestTransport {
        pub fn new(timeout: Duration) -> Result<Self, DocSourceError> {
            let client = reqwest::Client::builder()
                .timeout(timeout)
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .map_err(|_| DocSourceError::InvalidConfig("http client cannot be built"))?;
            Ok(Self { client })
        }
    }

    impl HttpTransport for ReqwestTransport {
        async fn send(&self, request: HttpRequest) -> Result<HttpResponse, DocSourceError> {
            let method = reqwest::Method::from_bytes(request.method.as_bytes())
                .map_err(|_| DocSourceError::Protocol("bad method"))?;
            let mut b = self.client.request(method, &request.url);
            for (k, v) in &request.headers {
                b = b.header(k.as_str(), v.as_str());
            }
            let resp = b.body(request.body).send().await.map_err(|e| {
                if e.is_timeout() {
                    DocSourceError::Timeout
                } else {
                    DocSourceError::Unavailable
                }
            })?;
            let status = resp.status().as_u16();
            let body = resp
                .bytes()
                .await
                .map_err(|_| DocSourceError::Unavailable)?
                .to_vec();
            Ok(HttpResponse { status, body })
        }
    }
}
