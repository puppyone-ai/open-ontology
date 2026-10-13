use tokio_postgres::Client;

use crate::config::Config;
use crate::error::Result;
use crate::tls;

pub async fn runtime(config: &Config) -> Result<Client> {
    connect(
        &config.host,
        config.port,
        &config.database,
        &config.runtime_user,
        &config.runtime_password,
        &config.ca_pem,
    )
    .await
}

pub async fn admin_db(config: &Config, database: &str) -> Result<Client> {
    connect(
        &config.host,
        config.port,
        database,
        &config.admin_user,
        &config.admin_password,
        &config.ca_pem,
    )
    .await
}

pub async fn target_runtime(config: &Config) -> Result<Client> {
    let database = config.target_database.as_deref().ok_or_else(|| crate::error::Error::new("config"))?;
    let user = config.target_runtime_user.as_deref().ok_or_else(|| crate::error::Error::new("config"))?;
    let password = config.target_runtime_password.as_deref().ok_or_else(|| crate::error::Error::new("config"))?;
    connect(
        &config.host,
        config.port,
        database,
        user,
        password,
        &config.ca_pem,
    )
    .await
}

/// Local Unix socket transport has no network hop; TCP always uses caller-CA TLS.
async fn connect(host:&str,port:u16,database:&str,user:&str,password:&str,ca:&[u8])->Result<Client> {
    if host.starts_with('/') && !host.contains("..") {
        let mut c=tokio_postgres::Config::new();
        c.host(host).port(port).dbname(database).user(user).password(password);
        let (client,connection)=c.connect(tokio_postgres::NoTls).await.map_err(crate::error::Error::sql)?;
        tokio::spawn(async move {let _=connection.await;});
        Ok(client)
    } else {tls::connect(host,port,database,user,password,ca).await}
}
