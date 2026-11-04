//! Only the labelled fixture runner supplies these references.
use std::{fs, process::Command, time::Duration};

use oo_connectors::transport::ssh;
use oo_stack::secret::SecretStore;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

async fn database_handshake(address: std::net::SocketAddr, postgres: bool) {
    let mut stream = tokio::net::TcpStream::connect(address).await.unwrap();
    if postgres {
        stream
            .write_all(&[0, 0, 0, 8, 4, 210, 22, 47])
            .await
            .unwrap();
        let mut reply = [0];
        tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply, [b'S']);
    } else {
        let mut reply = [0; 5];
        tokio::time::timeout(Duration::from_secs(10), stream.read_exact(&mut reply))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(reply[4], 10);
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "owned OpenSSH and source fixture; tests/ssh/run.py"]
async fn pin_auth_reconnect_and_shutdown() {
    let stack =
        oo_stack::parse(&fs::read_to_string(std::env::var("OO_SSH_STACK").unwrap()).unwrap())
            .unwrap();
    let source = &stack.sources[0];
    let store = SecretStore::open(std::env::var("OO_SSH_SECRETS").unwrap()).unwrap();
    let jump = source.ssh.as_ref().unwrap();
    assert_eq!(
        ssh::discover(&jump.host, jump.port).await.unwrap(),
        jump.host_key_sha256
    );
    assert_eq!(
        ssh::confirm_first_use(&jump.host, jump.port)
            .await
            .unwrap_err(),
        "SSH_PIN_REQUIRED"
    );

    let mut rejected = source.clone();
    rejected.ssh.as_mut().unwrap().host_key_sha256 = format!("SHA256:{}", "A".repeat(43));
    assert!(matches!(
        ssh::connect(&rejected, &store).await,
        Err("SSH_HOST_KEY_MISMATCH")
    ));
    let auth = rejected.ssh.as_mut().unwrap();
    auth.host_key_sha256 = jump.host_key_sha256.clone();
    auth.key = None;
    auth.password = Some(oo_stack::stack::SecretRef {
        secret_ref: "wrong-password".into(),
    });
    assert!(matches!(
        ssh::connect(&rejected, &store).await,
        Err("SSH_AUTH")
    ));

    let tunnel = ssh::connect(source, &store).await.unwrap().unwrap();
    let address = tunnel.address();
    let postgres = source.engine == "postgres";
    database_handshake(address, postgres).await;
    let container = std::env::var("OO_SSH_JUMP_CONTAINER").unwrap();
    let run = std::env::var("OO_SSH_RUN_ID").unwrap();
    let inspection = Command::new("docker")
        .args(["inspect", "--format", "{{json .Config.Labels}}", &container])
        .output()
        .unwrap();
    assert!(inspection.status.success());
    let labels: serde_json::Value = serde_json::from_slice(&inspection.stdout).unwrap();
    assert_eq!(labels["oo.lane"], "b");
    assert_eq!(labels["oo.run_id"], run);
    assert!(
        Command::new("docker")
            .args(["restart", &container])
            .output()
            .unwrap()
            .status
            .success()
    );
    // The same listener must open a new pinned session, not replay old bytes.
    database_handshake(address, postgres).await;
    let tunnel = match std::sync::Arc::try_unwrap(tunnel) {
        Ok(tunnel) => tunnel,
        Err(_) => panic!("unexpected tunnel owner"),
    };
    tunnel.shutdown().await;
    assert!(tokio::net::TcpStream::connect(address).await.is_err());
}
