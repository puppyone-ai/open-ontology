use oo_gateway::policy::{TlsEndpoint, connect_readonly_require_tls};

#[tokio::test]
async fn installed_all_in_one_session_is_readonly_and_rejects_admin() {
    if std::env::var("OO_ALL_IN_ONE_LIVE").as_deref() != Ok("1") {
        eprintln!("OO_ALL_IN_ONE_LIVE unset; owned PostgreSQL proof not run");
        return;
    }
    let password =
        std::fs::read_to_string(std::env::var("OO_W73_ROLE_PASSWORD_FILE").unwrap()).unwrap();
    let ca = std::fs::read(std::env::var("OO_W73_CA_FILE").unwrap()).unwrap();
    let mut endpoint = TlsEndpoint {
        host: std::env::var("OO_W73_PG_HOST").unwrap(),
        port: std::env::var("OO_W73_PG_PORT").unwrap().parse().unwrap(),
        database: std::env::var("OO_W73_TARGET_DATABASE").unwrap(),
        user: "oo_gateway_reader".to_owned(),
    };
    let (client, connection) = connect_readonly_require_tls(&endpoint, password.trim(), &ca)
        .await
        .unwrap();
    let driver = tokio::spawn(async move { connection.await.unwrap() });
    assert_eq!(
        client
            .query_one("SHOW default_transaction_read_only", &[])
            .await
            .unwrap()
            .get::<_, String>(0),
        "on"
    );
    for sql in [
        "INSERT INTO public.customer(id,contact) VALUES(9,'denied')",
        "UPDATE public.customer SET contact='denied'",
        "CREATE TABLE public.denied(id integer)",
    ] {
        assert_eq!(
            client
                .batch_execute(sql)
                .await
                .unwrap_err()
                .code()
                .unwrap()
                .code(),
            "25006"
        );
    }
    client
        .batch_execute("SET default_transaction_read_only=off")
        .await
        .unwrap();
    for sql in [
        "INSERT INTO public.customer(id,contact) VALUES(9,'denied')",
        "UPDATE public.customer SET contact='denied'",
        "CREATE TABLE public.denied(id integer)",
    ] {
        assert_eq!(
            client
                .batch_execute(sql)
                .await
                .unwrap_err()
                .code()
                .unwrap()
                .code(),
            "42501"
        );
    }
    drop(client);
    driver.await.unwrap();
    endpoint.user = "oo_fixture_admin".to_owned();
    assert!(
        connect_readonly_require_tls(&endpoint, password.trim(), &ca)
            .await
            .is_err()
    );
}
