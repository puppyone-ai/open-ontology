use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::allow::allow;
use crate::fault::Fault;

pub const UPSTREAM_PORT: u16 = 8317;

pub fn fixed_upstream() -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), UPSTREAM_PORT)
}

pub fn is_fixed_loopback(addr: SocketAddr) -> bool {
    addr.ip() == IpAddr::V4(Ipv4Addr::LOCALHOST)
}

pub fn upstream_url(upstream: SocketAddr, path: &str) -> Result<String, Fault> {
    if !is_fixed_loopback(upstream) || allow_path(path).is_none() {
        return Err(Fault::config());
    }
    Ok(format!("http://{upstream}{path}"))
}

fn allow_path(path: &str) -> Option<()> {
    allow("GET", path).or_else(|| allow("POST", path)).map(|_| ())
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::fixed_upstream;
    use super::upstream_url;

    #[test]
    fn production_upstream_is_exact() {
        assert_eq!(fixed_upstream().to_string(), "127.0.0.1:8317");
        assert_eq!(
            upstream_url(fixed_upstream(), "/v1/models").expect("url"),
            "http://127.0.0.1:8317/v1/models"
        );
        assert_eq!(
            upstream_url(fixed_upstream(), "/v1/chat/completions").expect("url"),
            "http://127.0.0.1:8317/v1/chat/completions"
        );
        let other: SocketAddr = "127.0.0.1:9".parse().expect("addr");
        assert!(upstream_url(other, "/v1/models").is_ok());
        let remote: SocketAddr = "8.8.8.8:8317".parse().expect("addr");
        assert!(upstream_url(remote, "/v1/models").is_err());
        assert!(upstream_url(fixed_upstream(), "/v1/other").is_err());
    }
}
