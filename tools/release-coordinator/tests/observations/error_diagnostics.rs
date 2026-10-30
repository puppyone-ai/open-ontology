use super::*;

#[test]
fn diagnostics_keep_only_categories_and_one_fixed_phase() {
    let secret = "SELECT private_customer_data; password=do-not-print";
    for kind in [
        std::io::ErrorKind::ConnectionReset,
        std::io::ErrorKind::BrokenPipe,
        std::io::ErrorKind::TimedOut,
        std::io::ErrorKind::UnexpectedEof,
        std::io::ErrorKind::Other,
    ] {
        let error = std::io::Error::new(kind, secret);
        let diagnostic = StoreError::QueryFailure(classify_source(&error))
            .at(Phase::NativeColumns).at(Phase::PreviousBinding);
        let text = diagnostic.to_string();
        assert!(text.contains(&format!("kind={kind:?}")));
        assert!(text.contains("phase=NativeColumns"));
        assert!(!text.contains("PreviousBinding"));
        assert!(!text.contains(secret));
        assert!(text.len() < 128);
        assert_eq!(diagnostic.reason(), "query");
    }
    let wrong_type = postgres_types::WrongType::new::<String>(postgres_types::Type::INT4);
    assert_eq!(classify_source(&wrong_type), QueryFailure::WrongType);
    assert_eq!(classify_source(&postgres_types::WasNull), QueryFailure::WasNull);
    assert_eq!(StoreError::Query {sqlstate:Some(secret.into())}.to_string(), "query unknown_sqlstate");
    assert_eq!(StoreError::Query {sqlstate:Some("42501".into())}.to_string(), "query 42501");
    assert_eq!(StoreError::Query {sqlstate:None}.to_string(), "query");
}

#[tokio::test]
#[ignore = "requires OO_G4_TEST_CONFIG for an owned local TLS PostgreSQL database"]
async fn owned_pg_diagnostics_classify_driver_errors_without_leaking_messages() {
    let config: serde_json::Value = serde_json::from_slice(
        &std::fs::read(std::env::var("OO_G4_TEST_CONFIG").unwrap()).unwrap(),
    ).unwrap();
    assert_eq!(config["host"], "127.0.0.1");
    assert!(config["database"].as_str().unwrap().starts_with("g4_observer_test_"));
    let password = std::fs::read_to_string(config["password_file"].as_str().unwrap()).unwrap();
    let ca = std::fs::read(config["ca_file"].as_str().unwrap()).unwrap();
    let (client, connection) = tokio_postgres::Config::new()
        .host("127.0.0.1").port(config["port"].as_u64().unwrap() as u16)
        .dbname(config["database"].as_str().unwrap()).user(config["user"].as_str().unwrap())
        .password(password.trim_end()).ssl_mode(tokio_postgres::config::SslMode::Require)
        .connect(crate::tls::rustls_connector(&ca).unwrap()).await.unwrap();
    let task = tokio::spawn(connection);
    client.batch_execute("SET default_transaction_read_only=on").await.unwrap();
    let row = client.query_one("SELECT 1::integer, NULL::text", &[]).await.unwrap();
    assert_eq!(query_error(row.try_get::<_, String>(0).unwrap_err()),
        StoreError::QueryFailure(QueryFailure::WrongType));
    assert_eq!(query_error(row.try_get::<_, String>(1).unwrap_err()),
        StoreError::QueryFailure(QueryFailure::WasNull));
    let error = client.query_one("SELECT 'private-password-must-not-leak'::integer", &[]).await.unwrap_err();
    let diagnostic = query_error(error).at(Phase::PreviousCandidate);
    assert_eq!(diagnostic.to_string(), "query 22P02 phase=PreviousCandidate");
    task.abort();
    let _ = task.await;
    let error = client.simple_query("SELECT 1").await.unwrap_err();
    assert!(error.is_closed());
    assert_eq!(query_error(error).to_string(), "query class=closed");
    println!("OWNED_PG_DIAGNOSTICS wrong_type=classified was_null=classified sqlstate=22P02 closed=classified raw_message_logged=false");
}
