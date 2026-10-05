//! Durable admit and complete records on a second TLS session.

use oo_gateway::query_executor::{Admission, AuditAdmission, CompletionRecord, ExecError};
use tokio::task::JoinHandle;
use tokio_postgres::types::ToSql;
use tokio_postgres::Client;

pub struct DbAudit {
    client: Client,
    _driver: JoinHandle<()>,
}

impl DbAudit {
    pub fn new(client: Client, driver: JoinHandle<()>) -> Self {
        Self {
            client,
            _driver: driver,
        }
    }
}

impl AuditAdmission for DbAudit {
    async fn admit(&mut self, admission: &Admission<'_>) -> Result<(), ExecError> {
        let pid = admission.backend_pid();
        self.client
            .execute(
                "INSERT INTO public.a203_audit \
                 (phase, original_sha256, executed_sha256, fingerprint_hex, role_name, project, policy_id, backend_pid, backend_start, status, returned_rows) \
                 VALUES ('admit', $1, $2, $3, $4, $5, $6, $7, $8, NULL, NULL)",
                &[
                    &admission.original_sha256() as &(dyn ToSql + Sync),
                    &admission.executed_sha256(),
                    &admission.fingerprint_hex(),
                    &admission.role(),
                    &admission.project(),
                    &admission.policy_id(),
                    &pid,
                    &admission.backend_start(),
                ],
            )
            .await
            .map_err(|_| ExecError::AuditUnavailable)?;
        Ok(())
    }

    async fn complete(&mut self, completion: &CompletionRecord) -> Result<(), ExecError> {
        let pid = completion.backend_pid();
        let status = completion.status().as_str();
        let returned = i32::try_from(completion.returned_rows()).unwrap_or(i32::MAX);
        let sqlstate = completion.sqlstate().unwrap_or("");
        let phase = format!("complete:{status}:{sqlstate}");
        self.client
            .execute(
                "INSERT INTO public.a203_audit \
                 (phase, original_sha256, executed_sha256, fingerprint_hex, role_name, project, policy_id, backend_pid, backend_start, status, returned_rows) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
                &[
                    &phase as &(dyn ToSql + Sync),
                    &completion.original_sha256(),
                    &completion.executed_sha256(),
                    &completion.fingerprint_hex(),
                    &completion.role(),
                    &completion.project(),
                    &completion.policy_id(),
                    &pid,
                    &completion.backend_start(),
                    &status,
                    &returned,
                ],
            )
            .await
            .map_err(|_| ExecError::AuditUnavailable)?;
        Ok(())
    }
}
