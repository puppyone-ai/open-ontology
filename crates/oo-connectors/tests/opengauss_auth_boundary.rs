//! Current driver admission blockers, not openGauss conformance passes.
//!
//! openGauss 6.0.0 pqcomm.h assigns SHA256=10 and SM3=13. PostgreSQL
//! assigns SASL=10. Keep these tests until a family-specific driver path is
//! implemented, then replace them with successful authenticated live probes.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

use sqlx::Connection;
use sqlx::postgres::{PgConnectOptions, PgConnection, PgSslMode};

async fn rejected_challenge(method: u32) -> String {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        socket
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let mut size = [0; 4];
        socket.read_exact(&mut size).unwrap();
        let size = u32::from_be_bytes(size) as usize;
        assert!((8..=4096).contains(&size));
        let mut startup = vec![0; size - 4];
        socket.read_exact(&mut startup).unwrap();
        assert_eq!(&startup[..4], &196608_u32.to_be_bytes());

        // The SHA256 challenge starts with a binary password-storage method,
        // not a NUL-terminated SASL mechanism name. Its leading zero is enough
        // to expose SQLx's misdispatch, before any password is needed.
        let mut body = method.to_be_bytes().to_vec();
        body.extend_from_slice(&2_u32.to_be_bytes());
        body.extend_from_slice(&[b'0'; 136]);
        let mut message = vec![b'R'];
        message.extend_from_slice(&((body.len() + 4) as u32).to_be_bytes());
        message.extend_from_slice(&body);
        socket.write_all(&message).unwrap();
        // A rejected challenge must not produce a password response.
        let mut reply = [0; 1];
        assert_eq!(socket.read(&mut reply).unwrap(), 0);
    });
    let options = PgConnectOptions::new()
        .host("127.0.0.1")
        .port(port)
        .username("fixture_reader")
        .database("postgres")
        .ssl_mode(PgSslMode::Disable);
    let result = tokio::time::timeout(Duration::from_secs(5), PgConnection::connect_with(&options))
        .await
        .expect("driver must reject promptly");
    let error = match result {
        Ok(_) => panic!("unimplemented authentication must not succeed"),
        Err(error) => error.to_string(),
    };
    server.join().unwrap();
    error
}

#[tokio::test]
async fn sha256_is_misdispatched_as_postgres_sasl() {
    assert!(
        rejected_challenge(10)
            .await
            .contains("unsupported SASL authentication mechanisms")
    );
}

#[tokio::test]
async fn sm3_is_not_implemented_by_the_postgres_driver() {
    assert!(
        rejected_challenge(13)
            .await
            .contains("unknown authentication method: 13")
    );
}
