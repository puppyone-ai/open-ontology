use tokio_postgres::Client;

use crate::config::Config;
use crate::error::Result;
use crate::tls;

pub async fn runtime(config: &Config) -> Result<Client> {
    tls::connect(
        &config.host,
        config.port,
        &config.database,
        &config.runtime_user,
        &config.runtime_password,
        &config.ca_pem,
    )
    .await
}
