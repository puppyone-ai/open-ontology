//! Gateway origin and the fixed `/v1/query` path.
//!
//! The base is an ASCII origin plus an optional mount prefix. This client
//! appends `/v1/query`. A base that already ends in that suffix is rejected.

use std::fmt;
use std::net::{Ipv4Addr, Ipv6Addr};

use reqwest::Url;

use crate::config::TransportMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EndpointError {
    Characters,
    Userinfo,
    Query,
    Fragment,
    Scheme,
    NotLoopback,
    AmbiguousPath,
    Origin,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Endpoint {
    url: Url,
}

impl Endpoint {
    pub(crate) fn parse(base: &str, transport: TransportMode) -> Result<Self, EndpointError> {
        if base.len() > 2_048 || base.bytes().any(|byte| !(0x21..=0x7e).contains(&byte)) {
            return Err(EndpointError::Characters);
        }
        if base.contains('%') || base.contains('\\') || base.contains('@') {
            return Err(EndpointError::Characters);
        }
        let mut url = Url::parse(base).map_err(|_| EndpointError::Origin)?;
        if !url.username().is_empty() || url.password().is_some() {
            return Err(EndpointError::Userinfo);
        }
        if url.query().is_some() {
            return Err(EndpointError::Query);
        }
        if url.fragment().is_some() {
            return Err(EndpointError::Fragment);
        }
        match url.scheme() {
            "https" => {}
            "http" if transport == TransportMode::LocalDevelopment && is_loopback(&url) => {}
            "http" => return Err(EndpointError::NotLoopback),
            _ => return Err(EndpointError::Scheme),
        }
        if url.host_str().is_none() {
            return Err(EndpointError::Origin);
        }
        // `Url` removes `.` and `..` before `path_segments` can see them.
        // Compare the caller's path with the parsed path and refuse the rewrite.
        if !path_unchanged(base, url.path()) {
            return Err(EndpointError::AmbiguousPath);
        }
        let path = query_path(&url)?;
        url.set_path(&path);
        url.set_query(None);
        url.set_fragment(None);
        if url.path() != path || url.query().is_some() || url.fragment().is_some() {
            return Err(EndpointError::AmbiguousPath);
        }
        Ok(Self { url })
    }

    pub(crate) fn url(&self) -> &Url {
        &self.url
    }

    pub(crate) fn as_str(&self) -> &str {
        self.url.as_str()
    }
}

fn query_path(url: &Url) -> Result<String, EndpointError> {
    let Some(segments) = url.path_segments() else {
        return Err(EndpointError::AmbiguousPath);
    };
    let raw: Vec<&str> = segments.collect();
    let mut parts = raw.clone();
    if parts.last().copied() == Some("") {
        parts.pop();
    }
    if parts.iter().any(|segment| segment.is_empty()) {
        return Err(EndpointError::AmbiguousPath);
    }
    for segment in &parts {
        if *segment == "." || *segment == ".." || !segment_ok(segment) {
            return Err(EndpointError::AmbiguousPath);
        }
    }
    let prefix = if parts.is_empty() {
        String::new()
    } else {
        format!("/{}", parts.join("/"))
    };
    if prefix == "/v1/query" || prefix.ends_with("/v1/query") {
        return Err(EndpointError::AmbiguousPath);
    }
    Ok(format!("{prefix}/v1/query"))
}

fn segment_ok(segment: &str) -> bool {
    segment
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'~' | b'-'))
}

fn path_unchanged(base: &str, parsed: &str) -> bool {
    let Some((_, rest)) = base.split_once("://") else {
        return false;
    };
    let raw = match rest.find(['/', '?', '#']) {
        Some(index) => &rest[index..],
        None => "",
    };
    let raw = raw.split(['?', '#']).next().unwrap_or("");
    if raw.is_empty() {
        parsed == "/"
    } else {
        raw == parsed
    }
}

fn is_loopback(url: &Url) -> bool {
    let Some(host) = url.host_str() else {
        return false;
    };
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // `host_str` puts IPv6 addresses inside brackets.
    let bare = host
        .strip_prefix('[')
        .and_then(|rest| rest.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = bare.parse::<Ipv4Addr>() {
        return ip.is_loopback();
    }
    if let Ok(ip) = bare.parse::<Ipv6Addr>() {
        return ip == Ipv6Addr::LOCALHOST;
    }
    false
}

impl fmt::Display for EndpointError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Characters => "rejected characters or encoding",
            Self::Userinfo => "userinfo",
            Self::Query => "query",
            Self::Fragment => "fragment",
            Self::Scheme => "scheme",
            Self::NotLoopback => "plaintext is only for an explicit loopback base",
            Self::AmbiguousPath => "ambiguous path",
            Self::Origin => "origin",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_https_appends_query_path() {
        let endpoint = Endpoint::parse("https://gateway.example/mount/", TransportMode::Production)
            .expect("base");
        assert_eq!(endpoint.as_str(), "https://gateway.example/mount/v1/query");
    }

    #[test]
    fn production_refuses_plaintext_loopback_and_userinfo() {
        assert_eq!(
            Endpoint::parse("http://127.0.0.1:9/", TransportMode::Production).unwrap_err(),
            EndpointError::NotLoopback
        );
        assert!(
            Endpoint::parse(
                "https://user:secret@gateway.example/",
                TransportMode::Production
            )
            .is_err()
        );
        assert!(
            Endpoint::parse(
                "https://gateway.example/v1/query",
                TransportMode::Production
            )
            .is_err()
        );
        assert!(
            Endpoint::parse(
                "https://gateway.example/a/%2e%2e/b",
                TransportMode::Production
            )
            .is_err()
        );
        assert_eq!(
            Endpoint::parse("https://gateway.example/a/../b", TransportMode::Production)
                .unwrap_err(),
            EndpointError::AmbiguousPath
        );
        assert!(Endpoint::parse("https://gateway.example/./b", TransportMode::Production).is_err());
        assert!(Endpoint::parse("https://gateway.example?x=1", TransportMode::Production).is_err());
        assert!(
            Endpoint::parse("https://gateway.example#frag", TransportMode::Production).is_err()
        );
        assert!(
            Endpoint::parse("https://gateway.example/a//b", TransportMode::Production).is_err()
        );
    }

    #[test]
    fn local_development_allows_only_loopback_plaintext() {
        let endpoint = Endpoint::parse("http://127.0.0.2:9", TransportMode::LocalDevelopment)
            .expect("loopback");
        assert_eq!(endpoint.as_str(), "http://127.0.0.2:9/v1/query");
        assert!(Endpoint::parse("http://[::1]/", TransportMode::LocalDevelopment).is_ok());
        assert!(Endpoint::parse("http://localhost/", TransportMode::LocalDevelopment).is_ok());
        assert!(Endpoint::parse("http://example.test/", TransportMode::LocalDevelopment).is_err());
    }
}
