//! Pinned SSH direct-tcpip forwarding. Never replay an interrupted DB stream.
use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use oo_stack::{
    secret::SecretStore,
    stack::{Source, Ssh, fingerprint},
};
use russh::{
    client,
    keys::{HashAlg, PrivateKeyWithHashAlg, PublicKeyOrCertificate},
};
use tokio::{
    net::TcpListener,
    task::JoinSet,
    time::{sleep, timeout},
};
use zeroize::Zeroizing;

type Result<T> = std::result::Result<T, &'static str>;
const DEADLINE: Duration = Duration::from_secs(10);
const CONNECTIONS: usize = 4;

// russh runs its protocol task separately. Close the OS socket even if a
// caller cancels while key exchange is stalled, before a handle is returned.
struct SocketGuard(std::net::TcpStream);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = self.0.shutdown(std::net::Shutdown::Both);
    }
}

async fn socket(host: &str, port: u16) -> Result<(tokio::net::TcpStream, SocketGuard)> {
    let socket = tokio::net::TcpStream::connect((host, port))
        .await
        .map_err(|_| "SSH_CONNECT")?
        .into_std()
        .map_err(|_| "SSH_CONNECT")?;
    let guard = SocketGuard(socket.try_clone().map_err(|_| "SSH_CONNECT")?);
    let stream = tokio::net::TcpStream::from_std(socket).map_err(|_| "SSH_CONNECT")?;
    Ok((stream, guard))
}

struct Session {
    handle: client::Handle<Pinned>,
    _socket: SocketGuard,
}

struct Pinned {
    expected: String,
    observed: Arc<Mutex<Option<String>>>,
}

impl client::Handler for Pinned {
    type Error = russh::Error;

    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let actual = key.public_key().fingerprint(HashAlg::Sha256).to_string();
        if let Ok(mut observed) = self.observed.lock() {
            *observed = Some(actual.clone());
        }
        // Certificates are not CA trust: pin the actual public key.
        Ok(actual == self.expected)
    }
}

fn config() -> Arc<client::Config> {
    Arc::new(client::Config {
        keepalive_interval: Some(Duration::from_secs(15)),
        keepalive_max: 3,
        ..Default::default()
    })
}

/// Discover without authenticating or trusting. The caller must display and
/// explicitly confirm this fingerprint, then persist it before calling connect.
pub async fn discover(host: &str, port: u16) -> Result<String> {
    let observed = Arc::new(Mutex::new(None));
    let handler = Pinned {
        expected: String::new(),
        observed: observed.clone(),
    };
    let _ = timeout(DEADLINE, async {
        let (stream, _guard) = socket(host, port).await?;
        let _ = client::connect_stream(config(), stream, handler).await;
        Ok::<_, &'static str>(())
    })
    .await;
    observed
        .lock()
        .map_err(|_| "SSH_HOST_KEY")?
        .clone()
        .ok_or("SSH_HOST_KEY")
}

/// TUI hook: refusal or non-TTY never establishes trust. The caller persists
/// the returned value in stack.yaml; this function never writes global SSH files.
pub async fn confirm_first_use(host: &str, port: u16) -> Result<String> {
    use std::io::{IsTerminal, Write};
    if !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
        return Err("SSH_PIN_REQUIRED");
    }
    let pin = discover(host, port).await?;
    eprint!("SSH host key {pin}. Verify out of band. Trust this key? Type yes: ");
    std::io::stderr().flush().map_err(|_| "SSH_CONFIRM_IO")?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|_| "SSH_CONFIRM_IO")?;
    if answer.trim() != "yes" {
        return Err("SSH_HOST_KEY_REJECTED");
    }
    Ok(pin)
}

struct Credentials {
    ssh: Ssh,
    secret: Zeroizing<String>,
}

async fn session(auth: &Credentials) -> Result<Session> {
    let observed = Arc::new(Mutex::new(None));
    let handler = Pinned {
        expected: auth.ssh.host_key_sha256.clone(),
        observed: observed.clone(),
    };
    let (stream, guard) = socket(&auth.ssh.host, auth.ssh.port).await?;
    let connection = client::connect_stream(config(), stream, handler).await;
    let mut handle = match connection {
        Ok(handle) => handle,
        Err(_) => {
            let observed = observed.lock().map_err(|_| "SSH_HOST_KEY")?;
            return Err(
                if observed
                    .as_ref()
                    .is_some_and(|key| key != &auth.ssh.host_key_sha256)
                {
                    "SSH_HOST_KEY_MISMATCH"
                } else {
                    "SSH_CONNECT"
                },
            );
        }
    };
    let authenticated = if auth.ssh.key.is_some() {
        let key = russh::keys::decode_secret_key(&auth.secret, None).map_err(|_| "SSH_KEY")?;
        handle
            .authenticate_publickey(
                &auth.ssh.user,
                PrivateKeyWithHashAlg::new(Arc::new(key), None),
            )
            .await
    } else {
        handle
            .authenticate_password(&auth.ssh.user, auth.secret.as_str())
            .await
    }
    .map_err(|_| "SSH_AUTH")?;
    if !authenticated.success() {
        let _ = handle
            .disconnect(russh::Disconnect::ByApplication, "", "")
            .await;
        return Err("SSH_AUTH");
    }
    Ok(Session {
        handle,
        _socket: guard,
    })
}

async fn retry_session(auth: &Credentials) -> Result<Session> {
    for attempt in 0..3 {
        let result = timeout(DEADLINE, session(auth))
            .await
            .unwrap_or(Err("SSH_TIMEOUT"));
        match result {
            Ok(handle) => return Ok(handle),
            Err(error @ ("SSH_CONNECT" | "SSH_TIMEOUT")) => {
                if attempt == 2 {
                    return Err(error);
                }
                sleep(Duration::from_millis(250 << attempt)).await;
            }
            Err(error) => return Err(error),
        }
    }
    unreachable!()
}

/// Last owner closes the listener and all active channels. No process-global
/// registry or detached forwarding workers survives the guard.
pub struct Tunnel {
    address: SocketAddr,
    target_host: String,
    target_port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl Tunnel {
    pub fn address(&self) -> SocketAddr {
        self.address
    }

    pub fn forwards(&self, host: &str, port: u16) -> bool {
        self.target_host == host && self.target_port == port
    }

    pub async fn shutdown(self) {
        self.task.abort();
        // Drop also aborts, so borrow the handle rather than moving it.
        let mut this = self;
        let _ = (&mut this.task).await;
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// Shared entry point for stack consumers. Only secret references are accepted;
/// resolve from the owner-private store before opening any network connection.
pub async fn connect(source: &Source, store: &SecretStore) -> Result<Option<Arc<Tunnel>>> {
    let Some(ssh) = &source.ssh else {
        return Ok(None);
    };
    // SQL Server does not yet separate its socket address from its TLS identity.
    if source.engine.split('-').next() == Some("sqlserver") {
        return Err("SQLSERVER_SSH_NOT_AVAILABLE");
    }
    if !fingerprint(&ssh.host_key_sha256) {
        return Err("SSH_PIN_REQUIRED");
    }
    if ssh.port == 0 || source.port == 0 || ssh.key.is_some() == ssh.password.is_some() {
        return Err("SSH_CONFIG");
    }
    let reference = ssh
        .key
        .as_ref()
        .or(ssh.password.as_ref())
        .ok_or("SSH_CONFIG")?;
    let auth = Arc::new(Credentials {
        ssh: ssh.clone(),
        secret: Zeroizing::new(store.read(&reference.secret_ref)?),
    });
    let initial = retry_session(&auth).await?;
    let listener = TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|_| "SSH_LISTEN")?;
    let address = listener.local_addr().map_err(|_| "SSH_LISTEN")?;
    let host = source.host.clone();
    let port = source.port;
    let task = tokio::spawn(async move {
        let mut channels = JoinSet::new();
        let mut initial = Some(initial);
        loop {
            tokio::select! {
                result = listener.accept(), if channels.len() < CONNECTIONS => {
                    let Ok((mut socket, _)) = result else { break };
                    let auth = auth.clone();
                    let host = host.clone();
                    let cached = initial.take();
                    channels.spawn(async move {
                        let handle = match cached {
                            Some(handle) if !handle.handle.is_closed() => handle,
                            _ => match retry_session(&auth).await {
                                Ok(handle) => handle,
                                Err(_) => return,
                            },
                        };
                        if let Ok(Ok(channel)) = timeout(DEADLINE,
                            handle.handle.channel_open_direct_tcpip(host, u32::from(port), "127.0.0.1", 0)
                        ).await {
                            let mut stream = channel.into_stream();
                            let _ = tokio::io::copy_bidirectional(&mut socket, &mut stream).await;
                        }
                        let _ = timeout(DEADLINE,
                            handle.handle.disconnect(russh::Disconnect::ByApplication, "", "")
                        ).await;
                    });
                }
                _ = channels.join_next(), if !channels.is_empty() => {}
            }
        }
    });
    Ok(Some(Arc::new(Tunnel {
        address,
        target_host: source.host.clone(),
        target_port: port,
        task,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn sqlserver_ssh_is_refused_before_secret_or_network_access() {
        let mut source = oo_stack::parse(include_str!("../../../oo-stack/example.stack.json"))
            .unwrap()
            .sources
            .remove(0);
        let directory = std::env::current_dir()
            .unwrap()
            .join(format!(".ssh-refusal-test-{}", std::process::id()));
        let store = SecretStore::create(&directory).unwrap();
        for engine in ["sqlserver", "sqlserver-2022"] {
            source.engine = engine.into();
            source.ssh = None;
            assert!(connect(&source, &store).await.unwrap().is_none());
            source.ssh = Some(
                serde_json::from_value(serde_json::json!({
                    "host": "unused.invalid", "user": "unused",
                    "key": {"secret_ref": "missing"},
                    "host_key_sha256": ""
                }))
                .unwrap(),
            );
            assert!(matches!(
                connect(&source, &store).await,
                Err("SQLSERVER_SSH_NOT_AVAILABLE")
            ));
        }
        std::fs::remove_dir(directory).unwrap();
    }

    #[tokio::test]
    async fn reconnect_is_bounded_and_backs_off() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = tokio::spawn(async move {
            for _ in 0..3 {
                drop(listener.accept().await.unwrap());
            }
        });
        let credentials = Credentials {
            ssh: Ssh {
                host: "127.0.0.1".into(),
                port,
                user: "fixture".into(),
                key: None,
                password: Some(oo_stack::stack::SecretRef {
                    secret_ref: "unused".into(),
                }),
                host_key_sha256: format!("SHA256:{}", "A".repeat(43)),
            },
            secret: Zeroizing::new("unused".into()),
        };
        let started = std::time::Instant::now();
        let result = timeout(Duration::from_secs(3), retry_session(&credentials))
            .await
            .unwrap();
        assert!(matches!(result, Err("SSH_CONNECT")));
        assert!(started.elapsed() >= Duration::from_millis(750));
        server.await.unwrap();
        assert_eq!(config().keepalive_interval, Some(Duration::from_secs(15)));
        assert_eq!(config().keepalive_max, 3);
    }

    #[tokio::test]
    async fn cancelling_stalled_key_exchange_closes_the_socket() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let (ready, received) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            stream
                .write_all(b"SSH-2.0-stalled-fixture\r\n")
                .await
                .unwrap();
            // More than the client identification: key exchange has started.
            stream.read_exact(&mut [0; 100]).await.unwrap();
            ready.send(()).unwrap();
            let mut remaining = Vec::new();
            stream.read_to_end(&mut remaining).await.unwrap();
        });
        let client = tokio::spawn(async move { discover("127.0.0.1", port).await });
        timeout(Duration::from_secs(2), received)
            .await
            .unwrap()
            .unwrap();
        client.abort();
        let _ = client.await;
        timeout(Duration::from_secs(2), server)
            .await
            .unwrap()
            .unwrap();
    }

    #[tokio::test]
    async fn handler_refuses_mismatch_without_secret_resolution() {
        let key = russh::keys::PublicKey::from_openssh(
            "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA",
        )
        .unwrap();
        let public = PublicKeyOrCertificate::from(key);
        let observed = Arc::new(Mutex::new(None));
        let mut handler = Pinned {
            expected: String::new(),
            observed: observed.clone(),
        };
        assert!(
            !client::Handler::check_server_key(&mut handler, &public)
                .await
                .unwrap()
        );
        handler.expected = observed.lock().unwrap().clone().unwrap();
        assert!(
            client::Handler::check_server_key(&mut handler, &public)
                .await
                .unwrap()
        );
    }
}
