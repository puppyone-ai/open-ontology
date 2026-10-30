//! Caller bounds. Caps can be lower than the public query contract and cannot
//! be higher. There is no result cache and no environment proxy.

use std::net::IpAddr;
use std::time::Duration;

use crate::QUERY_HTTP_MAX_BYTES;
use crate::error::{LocalError, QueryError};

const MAX_TIMEOUT: Duration = Duration::from_secs(180);

/// Leave time for a complete Gateway timeout receipt, not a transport race.
pub const QUERY_RESPONSE_RESERVE: Duration = Duration::from_millis(1_500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransportMode {
    /// HTTPS only. Plaintext is rejected, including loopback.
    Production,
    /// HTTP is accepted only for a loopback host. TLS verification stays on.
    LocalDevelopment,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Timeouts {
    pub connect: Duration,
    pub request: Duration,
    pub read: Duration,
    pub overall: Duration,
}

impl Timeouts {
    pub const fn production() -> Self {
        Self {
            connect: Duration::from_secs(5),
            request: Duration::from_secs(30),
            read: Duration::from_secs(30),
            overall: Duration::from_secs(30),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ByteCaps {
    pub request: usize,
    pub response: usize,
}

impl ByteCaps {
    pub const fn contract() -> Self {
        Self {
            request: QUERY_HTTP_MAX_BYTES,
            response: QUERY_HTTP_MAX_BYTES,
        }
    }
}

/// Extra attempts after the first response. Zero disables automatic retry.
///
/// The ceiling is 3. Only a complete `RATE_LIMITED` or `OVERLOADED` envelope
/// can use those attempts. This is not the rest of the SP09 retry matrix.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RetryPolicy {
    max_extra_attempts: u8,
    max_wait: Duration,
}

impl RetryPolicy {
    pub const fn none() -> Self {
        Self {
            max_extra_attempts: 0,
            max_wait: Duration::ZERO,
        }
    }

    pub const fn admission(max_extra_attempts: u8, max_wait: Duration) -> Option<Self> {
        if max_extra_attempts > 3 || max_wait.as_millis() > 30_000 {
            return None;
        }
        Some(Self {
            max_extra_attempts,
            max_wait,
        })
    }

    pub const fn admission_default() -> Self {
        Self {
            max_extra_attempts: 3,
            max_wait: Duration::from_secs(2),
        }
    }

    pub const fn max_extra_attempts(self) -> u8 {
        self.max_extra_attempts
    }

    pub const fn max_wait(self) -> Duration {
        self.max_wait
    }
}

/// PEM trust anchors supplied by the caller. They replace platform roots.
/// Hostname verification stays enabled. This client does not read files or
/// environment variables to find them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RootTrust {
    Platform,
    OnlyPem(Vec<u8>),
}

#[derive(Clone, PartialEq, Eq)]
pub struct ClientConfig {
    transport: TransportMode,
    timeouts: Timeouts,
    bytes: ByteCaps,
    retry: RetryPolicy,
    trust: RootTrust,
    connect_ip: Option<IpAddr>,
    interface: Option<String>,
    /// PEM holding the client certificate and its private key. `Debug` says
    /// only whether it is present.
    identity: Option<Vec<u8>>,
}

impl ClientConfig {
    pub fn production() -> Self {
        Self {
            transport: TransportMode::Production,
            timeouts: Timeouts::production(),
            bytes: ByteCaps::contract(),
            retry: RetryPolicy::admission_default(),
            trust: RootTrust::Platform,
            connect_ip: None,
            interface: None,
            identity: None,
        }
    }

    pub fn local_development() -> Self {
        Self {
            transport: TransportMode::LocalDevelopment,
            ..Self::production()
        }
    }

    pub fn transport(&self) -> TransportMode {
        self.transport
    }

    pub fn timeouts(&self) -> Timeouts {
        self.timeouts
    }

    pub fn byte_caps(&self) -> ByteCaps {
        self.bytes
    }

    pub fn retry_policy(&self) -> RetryPolicy {
        self.retry
    }

    /// Operator-only DNS override. Keeps the URL host, SNI and TLS verification.
    /// The endpoint must have a DNS hostname; its port is not overridden.
    pub fn with_connect_ip(mut self, ip: IpAddr) -> Self {
        self.connect_ip = Some(ip);
        self
    }

    /// Operator-only socket binding. Unsupported platforms fail at client creation;
    /// a missing interface fails at connection time, without an unbound fallback.
    pub fn with_interface(mut self, interface: String) -> Result<Self, QueryError> {
        if interface.is_empty()
            || interface.len() > 255
            || !interface.bytes().all(|byte| (0x21..=0x7e).contains(&byte))
        {
            return Err(QueryError::Local(LocalError::Config(
                "interface must be 1..=255 printable ASCII bytes without whitespace",
            )));
        }
        self.interface = Some(interface);
        Ok(self)
    }

    pub(crate) fn connect_ip(&self) -> Option<IpAddr> {
        self.connect_ip
    }

    pub(crate) fn interface(&self) -> Option<&str> {
        self.interface.as_deref()
    }

    pub fn with_timeouts(mut self, timeouts: Timeouts) -> Result<Self, QueryError> {
        for timeout in [
            timeouts.connect,
            timeouts.request,
            timeouts.read,
            timeouts.overall,
        ] {
            if timeout.is_zero() || timeout > MAX_TIMEOUT {
                return Err(QueryError::Local(LocalError::Config(
                    "timeout must be non-zero and at most 180s",
                )));
            }
        }
        self.timeouts = timeouts;
        Ok(self)
    }

    pub fn with_byte_caps(mut self, bytes: ByteCaps) -> Result<Self, QueryError> {
        if bytes.request == 0
            || bytes.response == 0
            || bytes.request > QUERY_HTTP_MAX_BYTES
            || bytes.response > QUERY_HTTP_MAX_BYTES
        {
            return Err(QueryError::Local(LocalError::Config(
                "byte cap must be inside the public 1 MiB contract",
            )));
        }
        self.bytes = bytes;
        Ok(self)
    }

    pub fn with_retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_trust(mut self, trust: RootTrust) -> Result<Self, QueryError> {
        if let RootTrust::OnlyPem(pem) = &trust
            && (pem.is_empty() || pem.len() > QUERY_HTTP_MAX_BYTES)
        {
            return Err(QueryError::Local(LocalError::Trust));
        }
        self.trust = trust;
        Ok(self)
    }

    /// Client certificate and private key for the gateway workload check.
    /// Empty, oversized, or NUL-bearing material is refused before a socket.
    pub fn with_identity(mut self, pem: Vec<u8>) -> Result<Self, QueryError> {
        if pem.is_empty() || pem.len() > QUERY_HTTP_MAX_BYTES || pem.contains(&0) {
            return Err(QueryError::Local(LocalError::Trust));
        }
        self.identity = Some(pem);
        Ok(self)
    }

    pub(crate) fn trust(&self) -> &RootTrust {
        &self.trust
    }

    pub(crate) fn identity(&self) -> Option<&[u8]> {
        self.identity.as_deref()
    }
}

impl std::fmt::Debug for ClientConfig {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ClientConfig")
            .field("transport", &self.transport)
            .field("timeouts", &self.timeouts)
            .field("bytes", &self.bytes)
            .field("retry", &self.retry)
            .field("trust", &self.trust)
            .field("connect_ip", &self.connect_ip)
            .field("interface", &self.interface)
            .field("identity", &self.identity.as_ref().map(|_| "present"))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_is_opt_in_and_interface_cannot_panic_the_connector() {
        let config = ClientConfig::production();
        assert_eq!(config.connect_ip(), None);
        assert_eq!(config.interface(), None);
        for interface in ["", "en0\0", "en 0", "\nen0", &"a".repeat(256)] {
            assert!(config.clone().with_interface(interface.to_owned()).is_err());
        }
        assert_eq!(
            config.with_connect_ip("::1".parse().unwrap()).connect_ip(),
            Some("::1".parse().unwrap())
        );
    }

    #[test]
    fn identity_is_redacted_and_rejected_when_empty_or_nul() {
        assert!(matches!(
            ClientConfig::production().with_identity(Vec::new()),
            Err(QueryError::Local(LocalError::Trust))
        ));
        assert!(matches!(
            ClientConfig::production().with_identity(vec![b'a', 0]),
            Err(QueryError::Local(LocalError::Trust))
        ));
        let pem = b"not-a-real-key-material".to_vec();
        let config = ClientConfig::production()
            .with_identity(pem.clone())
            .expect("bounded pem");
        let rendered = format!("{config:?}");
        assert!(rendered.contains("present"));
        assert!(!rendered.contains("not-a-real-key-material"));
        assert_ne!(config, ClientConfig::production());
    }
}
