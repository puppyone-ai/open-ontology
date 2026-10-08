//! Issuing adapter. The login role is the only authority this type accepts.
//! A principal, a role string, and a claims object are stored data.

use std::fmt;

use zeroize::Zeroize;

use crate::error::StoreError;
use crate::model::{Endpoint, IssueGrant, RevokeScope, SystemWall, WaitGate, WallClock};
use crate::sql;
use crate::store::{self, InstallReceipt, IssueReceipt, RevokeReceipt};

pub struct TrustedControlAdapter {
    endpoint: Endpoint,
    user: String,
    password: String,
    ca_pem: Vec<u8>,
}

impl fmt::Debug for TrustedControlAdapter {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("TrustedControlAdapter(redacted)")
    }
}

impl Drop for TrustedControlAdapter {
    fn drop(&mut self) {
        self.password.zeroize();
    }
}

impl TrustedControlAdapter {
    /// Checks the login name and stores the caller PEM. It does not open a connection.
    pub fn new(
        endpoint: Endpoint,
        user: impl Into<String>,
        password: impl Into<String>,
        ca_pem: impl Into<Vec<u8>>,
    ) -> Result<Self, StoreError> {
        let user = user.into();
        let mut password = password.into();
        if user != sql::ADMIN_ROLE {
            password.zeroize();
            return Err(StoreError::LoginRole);
        }
        if password.is_empty() || endpoint.host.is_empty() || endpoint.database.is_empty() || endpoint.port == 0 {
            password.zeroize();
            return Err(StoreError::Input);
        }
        Ok(Self {
            endpoint,
            user,
            password,
            ca_pem: ca_pem.into(),
        })
    }

    pub async fn install(&self) -> Result<InstallReceipt, StoreError> {
        store::install(&self.endpoint, &self.user, &self.password, &self.ca_pem).await
    }

    pub async fn issue(&self, grant: IssueGrant) -> Result<IssueReceipt, StoreError> {
        store::issue_with(
            &self.endpoint,
            &self.user,
            &self.password,
            &self.ca_pem,
            &grant,
            &SystemWall,
            &WaitGate::default(),
        )
        .await
    }

    /// Wall clock and wait gate are test seams. `wall` on the receipt is `injected` when the
    /// clock says so. This method does not certify `issue`.
    pub async fn issue_for_test(
        &self,
        grant: IssueGrant,
        clock: &dyn WallClock,
        gate: WaitGate,
    ) -> Result<IssueReceipt, StoreError> {
        store::issue_with(
            &self.endpoint,
            &self.user,
            &self.password,
            &self.ca_pem,
            &grant,
            clock,
            &gate,
        )
        .await
    }

    pub async fn revoke(&self, scope: RevokeScope) -> Result<RevokeReceipt, StoreError> {
        store::revoke_with(
            &self.endpoint,
            &self.user,
            &self.password,
            &self.ca_pem,
            &scope,
            &SystemWall,
            &WaitGate::default(),
        )
        .await
    }

    /// Same seam rule as `issue_for_test`. Production `revoke` does not wait.
    pub async fn revoke_for_test(
        &self,
        scope: RevokeScope,
        clock: &dyn WallClock,
        gate: WaitGate,
    ) -> Result<RevokeReceipt, StoreError> {
        store::revoke_with(
            &self.endpoint,
            &self.user,
            &self.password,
            &self.ca_pem,
            &scope,
            clock,
            &gate,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::TrustedControlAdapter;
    use crate::error::StoreError;
    use crate::model::Endpoint;
    use zeroize::Zeroize;

    #[test]
    fn runtime_login_is_refused_before_connect_and_debug_redacts_the_password() {
        let endpoint = Endpoint {
            host: "invalid".to_owned(),
            port: 1,
            database: "none".to_owned(),
        };
        let mut password = "super-secret-password".to_owned();
        let refused = TrustedControlAdapter::new(endpoint.clone(), "oo_token_runtime", &password, b"ca");
        assert!(matches!(refused, Err(StoreError::LoginRole)), "runtime login was accepted");
        let adapter = TrustedControlAdapter::new(endpoint, "oo_token_admin", &password, b"ca").expect("admin login");
        let debug = format!("{adapter:?}");
        assert_eq!(debug, "TrustedControlAdapter(redacted)");
        assert!(!debug.contains("super-secret-password"), "debug contained the password");
        password.zeroize();
    }
}
